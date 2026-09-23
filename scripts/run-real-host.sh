#!/usr/bin/env bash
# 用上游仓库里的真实 Host 启动外壳。
#
# 前置条件（都在上游仓库内完成）：
#   1. corepack pnpm install   依赖走镜像，见 docs/roadmap.md 的「依赖获取」
#   2. corepack pnpm run build 构建 Host 与 Web 前端
#   3. 准备运行时树：调用上游的 prepareDevelopmentProject
#   4. 准备载荷：调用上游的 preparePrimaryRuntime
#
# 环境变量：
#   DSH_UPSTREAM  上游仓库根目录（默认 ~/op/deepseek-harness）
#   DSH_HOME      Harness 数据根目录；未设置时用产品默认的 ~/.dsh
#   DSH_APP       外壳可执行文件（默认用 CARGO_TARGET_DIR 下的 debug 构建）
set -euo pipefail

UPSTREAM="${DSH_UPSTREAM:-$HOME/op/deepseek-harness}"
TARGET_DIR="${CARGO_TARGET_DIR:-$(cd "$(dirname "$0")/.." && pwd)/src-tauri/target}"
APP="${DSH_APP:-$TARGET_DIR/debug/dsh-desktop}"

if [ ! -x "$APP" ]; then
  echo "找不到外壳可执行文件：$APP" >&2
  echo "先构建：cargo build --manifest-path src-tauri/Cargo.toml" >&2
  exit 1
fi

# 上游按 `${os}-${arch}` 命名打包目标，与 npm_config_platform/arch 一致。
TARGET="$(node -p "const p = process.platform; (p === 'darwin' ? 'mac' : p === 'win32' ? 'win' : p) + '-' + process.arch")"
RUNTIME="$UPSTREAM/apps/desktop/.desktop-build/development/project"
PRIMARY="$UPSTREAM/apps/desktop/.desktop-build/targets/$TARGET/runtime/primary-runtime"

for required in \
  "$RUNTIME/node_modules/@deepseek-ai/dsh-desktop-host/lib/index.js" \
  "$PRIMARY/runtime.json"
do
  if [ ! -e "$required" ]; then
    echo "缺少 $required" >&2
    echo "先在上游仓库完成依赖安装、构建与运行时准备，见本脚本头部说明。" >&2
    exit 1
  fi
done

# Host 不传 argv[4] 时会推导到 `<runtimeDir>/../runtime/primary-runtime`，
# 而上游的载荷落在 targets/<target>/runtime 下，两者不一致，因此显式指定。
exec env \
  DSH_HOST_ENTRY="$RUNTIME/node_modules/@deepseek-ai/dsh-desktop-host/lib/index.js" \
  DSH_HOST_RUNTIME="$RUNTIME" \
  DSH_HOST_PRIMARY_RUNTIME="$PRIMARY" \
  "$APP"
