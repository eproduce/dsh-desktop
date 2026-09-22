#!/usr/bin/env bash
# 端到端验证外壳与 Host 的完整链路。
#
# 用 tests/fake-host.mjs 代替真实 Host，因此不需要 dsh 运行时、模型密钥或网络。
# 覆盖四件事：Host 就绪上报 → 导航到 Host 源 → 桥接与启动注入可用 → 关窗后优雅收尾。
#
# 环境变量：
#   DSH_E2E_PORT        假 Host 端口（默认 19389，避开真实 Host 的 19387）
#   DSH_E2E_RUNTIME     传给 Host 的运行时目录（默认新建临时目录）
#   DSH_E2E_CARGO_ARGS  追加给 cargo build 的参数，例如 --offline
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/src-tauri/target}"
BIN="$TARGET_DIR/debug/dsh-desktop"
PORT="${DSH_E2E_PORT:-19389}"
RUNTIME_DIR="${DSH_E2E_RUNTIME:-$(mktemp -d)}"
STOP_LOG="$(mktemp)"
APP_LOG="$(mktemp)"

fail() {
  echo "e2e 失败：$*" >&2
  echo "外壳日志（${APP_LOG}）：" >&2
  sed 's/^/  /' "$APP_LOG" >&2 || true
  exit 1
}

# 无论成败都不留下孤儿进程。
cleanup() {
  pkill -f "$BIN" 2>/dev/null || true
  pkill -f 'host-bridge.cjs' 2>/dev/null || true
  pkill -f 'fake-host.mjs' 2>/dev/null || true
}
trap cleanup EXIT
cleanup

echo "== 构建 =="
cargo build --manifest-path "$ROOT/src-tauri/Cargo.toml" ${DSH_E2E_CARGO_ARGS:-} || fail "构建失败"
if [ ! -x "$BIN" ]; then fail "未找到可执行文件 $BIN"; fi

echo "== 启动外壳（虚拟 Host，端口 ${PORT}） =="
DSH_FAKE_HOST_STOP_LOG="$STOP_LOG" \
DSH_FAKE_HOST_PORT="$PORT" \
DSH_HOST_ENTRY="$ROOT/tests/fake-host.mjs" \
DSH_HOST_RUNTIME="$RUNTIME_DIR" \
  "$BIN" > "$APP_LOG" 2>&1 &

# 假 Host 在结果就绪前对 /last-report 返回 503，因此可以直接靠 curl 重试。
# 服务起来前的连接拒绝是重试循环的正常噪声，失败时统一报错，因此丢弃 curl 的 stderr。
REPORT="$(curl -fsS --retry 25 --retry-delay 1 --retry-all-errors -m 40 "http://127.0.0.1:$PORT/last-report" 2>/dev/null)" \
  || fail "未取到工作区回传"

echo "$REPORT" | python3 -c '
import json, sys
facts = json.load(sys.stdin)
assert facts.get("bootOk") is True, "boot 未被放行：" + str(facts.get("bootError"))
assert facts.get("hostStatusOk") is True, "host_status 未被放行：" + str(facts.get("hostStatusError"))
assert facts.get("platform"), "未注入 html[data-platform]"
assert facts.get("hasBoot") is True, "未注入 dshDesktopBoot"
assert facts.get("hasDirectoryPicker") is True, "未注入 __DSH_DIRECTORY_PICKER__"
boot = facts.get("boot") or {}
assert boot.get("streamBaseUrl"), "boot 未返回 Host 地址"
assert boot.get("injections"), "boot 未返回启动注入数据"
print("  工作区 origin =", facts["origin"])
print("  平台标记 =", facts["platform"], "注入数据 =", boot["injections"])
print("  navigator.languages =", facts.get("navigatorLanguages"), "| navigator.language =", facts.get("navigatorLanguage"))
' || fail "工作区探测结果不符合预期"

# 读取最近一次上报里的全屏标记；未设置时 Python 打印 None。
read_fullscreen() {
  curl -fsS -m 5 "http://127.0.0.1:$PORT/last-report" 2>/dev/null \
    | python3 -c 'import json,sys; print(json.load(sys.stdin).get("fullscreen"))' 2>/dev/null || echo ''
}

# 轮询到标记等于期望值，或超时后返回当前值。
wait_fullscreen() {
  local want="$1" got=""
  for _ in $(seq 1 20); do
    got="$(read_fullscreen)"
    if [ "$got" = "$want" ]; then echo "$got"; return 0; fi
    sleep 0.5
  done
  echo "$got"
}

# 读取窗口的全屏属性；读取失败时输出空串。
read_ax_fullscreen() {
  osascript -e "tell application \"System Events\" to tell (first process whose unix id is $1) to get value of attribute \"AXFullScreen\" of window \"DeepSeek Harness\"" \
    2>/dev/null || echo ''
}

# 切换窗口全屏，并读回确认切换真的生效。
#
# macOS 的全屏进出是动画：动画未结束时再次切换会被忽略。只看「设置动作是否成功」
# 会在动画窗口期误判为已生效，因此这里以读回值为准，反复尝试到一致为止。
# 返回非零表示该窗口无法进入期望状态，调用方据此跳过断言。
set_fullscreen() {
  local pid="$1" want="$2" got=""
  for _ in $(seq 1 12); do
    got="$(read_ax_fullscreen "$pid")"
    if [ "$got" = "$want" ]; then return 0; fi
    osascript -e "tell application \"System Events\" to tell (first process whose unix id is $pid) to set value of attribute \"AXFullScreen\" of window \"DeepSeek Harness\" to $want" \
      >/dev/null 2>&1 || true
    sleep 0.5
  done
  return 1
}

# 按窗口名关窗；全屏下关闭按钮不可达，因此先确保处于非全屏。
close_window() {
  local pid="$1"
  set_fullscreen "$pid" false || true
  for _ in $(seq 1 8); do
    osascript -e "tell application \"System Events\" to tell (first process whose unix id is $pid) to perform action \"AXPress\" of button 1 of window \"DeepSeek Harness\"" \
      >/dev/null 2>&1 || true
    if ! pgrep -f "$BIN" >/dev/null; then return 0; fi
    sleep 0.5
  done
  return 1
}

if [ "$(uname)" = "Darwin" ]; then
  PID="$(pgrep -f "$BIN" | head -1)"

  echo "== 进入全屏，验证 html[data-fullscreen] =="
  if set_fullscreen "$PID" true; then
    GOT="$(wait_fullscreen true)"
    if [ "$GOT" = "true" ]; then echo "  进入全屏后标记已生效"; else fail "进入全屏后标记为 ${GOT}，期望 true"; fi

    echo "== 退出全屏，验证标记被清除 =="
    if set_fullscreen "$PID" false; then
      GOT="$(wait_fullscreen None)"
      if [ "$GOT" = "None" ]; then echo "  退出全屏后标记已清除"; else fail "退出全屏后标记为 ${GOT}，期望 None"; fi
    else
      echo "  提示：窗口无法退出全屏，跳过标记清除断言"
    fi
  else
    echo "  提示：无法通过脚本切换全屏，跳过该断言"
  fi

  echo "== 关窗，验证优雅收尾 =="
  close_window "$PID" || echo "  提示：无法通过脚本关窗，后续断言可能因外壳仍在运行而失败"
else
  echo "== 非 macOS：跳过关窗，直接结束进程 =="
  cleanup
fi

for _ in $(seq 1 40); do
  if ! pgrep -f "$BIN" >/dev/null; then break; fi
  sleep 0.5
done

if pgrep -f "$BIN" >/dev/null; then fail "外壳未退出"; fi
if pgrep -f 'host-bridge.cjs' >/dev/null; then fail "桥接进程残留"; fi
if pgrep -f 'fake-host.mjs' >/dev/null; then fail "Host 进程残留"; fi

grep -q '收到 shutdown' "$STOP_LOG" || fail "Host 未收到 shutdown，可能是被强杀"
grep -q '已发送 shutdown-complete' "$STOP_LOG" || fail "Host 未回应 shutdown-complete"
COUNT="$(grep -c '已发送 shutdown-complete' "$STOP_LOG")"
if [ "$COUNT" != "1" ]; then fail "shutdown-complete 发送了 ${COUNT} 次，应为 1 次"; fi

# 假 Host 上报一个本外壳尚未实现的上游事件（账号平台窗口用的 platform-session）。
# 外壳必须把它记成诊断：静默丢弃会让「上游没发」与「外壳没实现」无法区分。
grep -q '忽略无法识别的 Host 上报.*platform-session' "$APP_LOG" \
  || fail "外壳未把未实现的上游事件记入诊断"

echo "== 全部通过 =="
echo "  收尾握手："
sed 's/^/    /' "$STOP_LOG"
