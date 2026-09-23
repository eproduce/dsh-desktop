#!/usr/bin/env bash
# 用上游的真实 Host 冒烟检查外壳：真实工作区是否真的能在本外壳里启动，以及外壳是否稳定。
#
# 与 scripts/e2e.sh 互补。那个用 tests/fake-host.mjs，不依赖上游构建，覆盖协议细节与
# 生命周期；这个用真实 Host，覆盖只有它才会暴露的问题：注入表被应用两次、认证 cookie
# 不回送、凭据引导与桌面标记的相互作用。
#
# 前置条件与 scripts/run-real-host.sh 相同（上游已完成依赖安装、构建与运行时准备）。
#
# 环境变量：
#   DSH_SMOKE_SECONDS  观察时长秒数，默认 25
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PACKAGE_ROOT="$(cd "$ROOT/.." && pwd)"
WATCH_SECONDS="${DSH_SMOKE_SECONDS:-25}"
APP_LOG="$(mktemp)"

fail() {
  echo "冒烟失败：$*" >&2
  echo "外壳日志（${APP_LOG}）：" >&2
  sed 's/^/  /' "$APP_LOG" >&2 || true
  echo "相关进程：" >&2
  ps -A -o pid,ppid,stat,command 2>/dev/null \
    | grep -E 'dsh-desktop|host-bridge|dsh-desktop-host' \
    | grep -v grep | sed 's/^/  /' >&2 || echo "  （无）" >&2
  exit 1
}

# 无论成败都不留下孤儿进程。
cleanup() {
  pkill -f 'host-bridge.cjs' 2>/dev/null || true
  pkill -f 'dsh-desktop-host/lib' 2>/dev/null || true
  pkill -f 'debug/dsh-desktop' 2>/dev/null || true
}
trap cleanup EXIT
cleanup

echo "== 启动外壳（真实 Host） =="
bash "$ROOT/scripts/run-real-host.sh" > "$APP_LOG" 2>&1 &
APP_PID=$!

# Host 起来后会在环回地址监听。这是「工作区真的启动了」的正面信号：单看外壳日志只能
# 确认「没有报错」，无法区分「已就绪」与「还在启动」。
#
# 匹配 Host 必须带上 `--expose-internals`：桥接进程的命令行里同样带着 Host 入口路径，
# 只按路径匹配会先命中桥接，而它不监听任何端口。
HOST_READY=""
for _ in $(seq 1 "$WATCH_SECONDS"); do
  if ! kill -0 "$APP_PID" 2>/dev/null; then fail "外壳进程已退出"; fi
  HOST_PID="$(pgrep -f 'expose-internals.*dsh-desktop-host' | head -1 || true)"
  if [ -n "$HOST_PID" ] && lsof -a -p "$HOST_PID" -nP -iTCP -sTCP:LISTEN 2>/dev/null | grep -q LISTEN; then
    HOST_READY="$(lsof -a -p "$HOST_PID" -nP -iTCP -sTCP:LISTEN 2>/dev/null | awk 'NR==2 {print $9}')"
    break
  fi
  sleep 1
done
[ -n "$HOST_READY" ] || fail "在 ${WATCH_SECONDS} 秒内未看到 Host 监听，工作区可能没起来"
echo "  Host 监听于 $HOST_READY"

# 再观察一段，确认启动过程没有在半途失败。
for _ in $(seq 1 10); do
  sleep 1
done
kill -0 "$APP_PID" 2>/dev/null || fail "外壳在观察期内退出"

# 渲染层启动失败会经 boot_failed 上报，是客户端插件未激活、认证失败等问题的统一出口。
if grep -q '渲染层启动失败' "$APP_LOG"; then
  grep '渲染层启动失败' "$APP_LOG" | head -5 >&2
  fail "渲染层上报了启动失败"
fi

echo "  无渲染层启动失败上报，外壳稳定运行"
echo "== 通过 =="
