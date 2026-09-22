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
boot = facts.get("boot") or {}
assert boot.get("streamBaseUrl"), "boot 未返回 Host 地址"
assert boot.get("injections"), "boot 未返回启动注入数据"
print("  工作区 origin =", facts["origin"])
print("  平台标记 =", facts["platform"], "注入数据 =", boot["injections"])
' || fail "工作区探测结果不符合预期"

if [ "$(uname)" = "Darwin" ]; then
  echo "== 关窗，验证优雅收尾 =="
  PID="$(pgrep -f "$BIN" | head -1)"
  osascript -e "tell application \"System Events\" to tell (first process whose unix id is $PID) to perform action \"AXPress\" of button 1 of window \"DeepSeek Harness\"" \
    >/dev/null 2>&1 || echo "  提示：无法通过脚本关窗，后续断言可能因外壳仍在运行而失败"
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

echo "== 全部通过 =="
echo "  收尾握手："
sed 's/^/    /' "$STOP_LOG"
