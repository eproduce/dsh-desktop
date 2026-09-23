//! 桌面 profile 的初始化。
//!
//! 上游把 profile 的创建交给外壳：`apps/desktop-host` 直接调用 `loadProfileDirectory`
//! （`apps/desktop-host/src/index.ts`），绕过了 `loadProfile` 里「按名称查内置模板」
//! 的兜底，而 `PROFILE_TEMPLATES` 中也没有 `desktop` 这一项。因此清单不存在时 Host
//! 会以 `failed to read profile manifest` 退出，外壳必须先把它建好。
//!
//! 这里镜像上游 `initProfile`（`packages/boot/app-boot/src/profile.ts`）与
//! `apps/desktop/src/project-manager.ts` 的 `createPluginProfile`：同样只在文件缺失时
//! 写入（已存在的 profile 一律不动），同样使用 web 模板的 bundle 列表。上游改动模板
//! 时这里需要同步。

use std::fs;
use std::path::Path;

/// 用户补丁层文件名。
const PROFILE_PATCH_FILENAME: &str = "cordis.patch.yml";

/// 桌面 profile 的 bundle 层列表，取自上游 `PROFILE_TEMPLATES.web`。
const DESKTOP_PROFILE_BUNDLES: [&str; 2] = ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"];

/// 用户补丁层模板，与上游逐字一致。
///
/// 保留上游原文是为了辨认「尚未被修改」的 profile，见 {@link migrate_patch_layer}。
const UPSTREAM_PATCH_TEMPLATE: &str = "\
# Your patch layer for this dsh profile, applied after every bundle layer:
# a top-level YAML array of loader patch entries (id-targeted config
# overrides, disables, and insert lists; `!!js` expressions allowed).
[]
";

/// 补丁层里的外壳策略条目，追加到用户已有内容之后。
const PATCH_POLICY_ENTRY: &str = "\
- id: ui-settings-models
  config:
    credentialOnboarding: false
";

/// 本外壳为新建 profile 写入的补丁层：上游模板加一条外壳策略覆盖。
///
/// Electron 用欢迎窗口承接凭据设置，因此靠 `dshDesktop` 标记抑制应用内的模型凭据
/// 引导；本外壳没有那个窗口，若不关闭引导，用户就没有地方配置模型与 API。上游为这
/// 类原生壳准备了该开关（见 `ui-settings-models` 的 README）。设置页的模型与 API
/// 配置不受影响，只是不再弹引导。
const PROFILE_PATCH_TEMPLATE: &str = "\
# Your patch layer for this dsh profile, applied after every bundle layer:
# a top-level YAML array of loader patch entries (id-targeted config
# overrides, disables, and insert lists; `!!js` expressions allowed).
- id: ui-settings-models
  config:
    credentialOnboarding: false
";

/// pnpm 设置，与上游逐字一致。
///
/// 提升式链接让树外插件拿到扁平的 node_modules，其缺失的 peer（cordis 等）走运行时
/// 解析，于是每个插件共用安装里的同一个 cordis 实例而不是各自复制一份。pnpm 10 起从
/// `pnpm-workspace.yaml` 读这些设置，而不是 `.npmrc`。
const PROFILE_PNPM_WORKSPACE: &str = "\
packages:
  - .

nodeLinker: hoisted
autoInstallPeers: false
";

/// 建立桌面 profile 缺失的文件。
///
/// 已存在的文件一律保持原样，因此重复调用是幂等的，也不会覆盖用户改过的补丁层或
/// pnpm 设置。上游同样以此保证「首次打开才初始化」的语义。
///
/// @param dir - profile 目录，即传给 Host 的项目目录。
/// @returns 写盘失败时返回错误，由调用方决定如何呈现。
pub fn ensure_profile(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let manifest = dir.join("package.json");
    if !manifest.exists() {
        fs::write(manifest, manifest_json(dir))?;
    }
    let patch = dir.join(PROFILE_PATCH_FILENAME);
    if !patch.exists() {
        fs::write(patch, PROFILE_PATCH_TEMPLATE)?;
    }
    let workspace = dir.join("pnpm-workspace.yaml");
    if !workspace.exists() {
        fs::write(workspace, PROFILE_PNPM_WORKSPACE)?;
    }
    ensure_patch_policy(dir)
}

/// 确保补丁层带有本外壳的策略覆盖，且不破坏用户已有内容。
///
/// 上游把 `credentialOnboarding` 的开关放在这个补丁层里（`ui-settings-models` 的
/// 插件行）。三种情形分别处理：
///
/// - 已有该插件的条目：不动。用户若在其后另加一条同 id 的覆盖，仍可自行改回。
/// - 内容是上游空模板（顶层空数组 `[]`）：整体换成带覆盖的版本，否则追加会产生
///   无效 YAML。
/// - 其余情形：把覆盖作为新条目追加。这是纯增加的改动，用户自己的条目与注释都保留
///   ——应用自己也会往这个文件写状态（例如确认内测声明后的 `welcomeNoticeVersion`）。
fn ensure_patch_policy(dir: &Path) -> std::io::Result<()> {
    let path = dir.join(PROFILE_PATCH_FILENAME);
    let Ok(existing) = fs::read_to_string(&path) else {
        return Ok(());
    };
    // 以包名是否出现作为判据：比匹配具体 YAML 写法更宽松，宁可少改也不误判。
    if existing.contains("ui-settings-models") {
        return Ok(());
    }
    if existing.replace("\r\n", "\n") == UPSTREAM_PATCH_TEMPLATE {
        fs::write(path, PROFILE_PATCH_TEMPLATE)?;
        return Ok(());
    }
    let mut appended = existing;
    if !appended.ends_with('\n') {
        appended.push('\n');
    }
    appended.push_str(PATCH_POLICY_ENTRY);
    fs::write(path, appended)
}

/// profile 清单内容。
///
/// 名称与上游一样由目录名派生。用 JSON 序列化而不是拼接字符串，这样目录名里有需要
/// 转义的字符时也不会写出无效清单。
fn manifest_json(dir: &Path) -> String {
    let name = dir
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("desktop");
    let manifest = serde_json::json!({
        "name": format!("dsh-profile-{name}"),
        "private": true,
        "dependencies": {},
        "dsh": { "profile": { "bundles": DESKTOP_PROFILE_BUNDLES } },
    });
    let mut text = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string());
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在临时目录里跑一段逻辑；目录随作用域结束被删除。
    fn with_temp_dir(body: impl FnOnce(&Path)) {
        let dir = std::env::temp_dir().join(format!(
            "dsh-profile-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        body(&dir);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_every_file_a_new_profile_needs() {
        with_temp_dir(|dir| {
            let profile = dir.join("desktop");
            ensure_profile(&profile).expect("初始化应当成功");
            let manifest = fs::read_to_string(profile.join("package.json")).expect("清单应当存在");
            assert!(manifest.contains("dsh-profile-desktop"), "{manifest}");
            assert!(manifest.contains("@deepseek-ai/dsh-base"), "{manifest}");
            assert!(manifest.contains("@deepseek-ai/dsh-web-app"), "{manifest}");
            assert!(
                profile.join(PROFILE_PATCH_FILENAME).exists(),
                "补丁层应当存在"
            );
            assert!(
                profile.join("pnpm-workspace.yaml").exists(),
                "pnpm 设置应当存在"
            );
            // 没有欢迎窗口的壳必须关掉应用内引导，否则用户无处配置模型与 API。
            let patch = fs::read_to_string(profile.join(PROFILE_PATCH_FILENAME)).expect("应当可读");
            assert!(patch.contains("credentialOnboarding: false"), "{patch}");
        });
    }

    #[test]
    fn migrates_an_untouched_upstream_patch_layer() {
        with_temp_dir(|dir| {
            let profile = dir.join("desktop");
            // 上游 initProfile 写出的原文：本外壳早期版本创建的 profile 就是这一份。
            fs::create_dir_all(&profile).expect("创建目录应当成功");
            fs::write(
                profile.join(PROFILE_PATCH_FILENAME),
                UPSTREAM_PATCH_TEMPLATE,
            )
            .expect("写入应当成功");
            ensure_profile(&profile).expect("初始化应当成功");
            let patch = fs::read_to_string(profile.join(PROFILE_PATCH_FILENAME)).expect("应当可读");
            assert!(patch.contains("credentialOnboarding: false"), "{patch}");
        });
    }

    #[test]
    fn appends_the_policy_without_disturbing_existing_entries() {
        with_temp_dir(|dir| {
            let profile = dir.join("desktop");
            fs::create_dir_all(&profile).expect("创建目录应当成功");
            // 应用自己会往这个文件写状态，例如确认内测声明后的欢迎须知版本。
            let existing =
                "- id: ui-settings-general\n  config:\n    welcomeNoticeVersion: 2026-08-13.1\n";
            fs::write(profile.join(PROFILE_PATCH_FILENAME), existing).expect("写入应当成功");
            ensure_profile(&profile).expect("初始化应当成功");
            let patch = fs::read_to_string(profile.join(PROFILE_PATCH_FILENAME)).expect("应当可读");
            assert!(patch.starts_with(existing), "已有条目应当原样保留：{patch}");
            assert!(patch.contains("credentialOnboarding: false"), "{patch}");
        });
    }

    #[test]
    fn leaves_a_user_supplied_override_alone() {
        with_temp_dir(|dir| {
            let profile = dir.join("desktop");
            fs::create_dir_all(&profile).expect("创建目录应当成功");
            // 用户自己写了一条同 id 的覆盖：不再追加，让他能自行改回引导的行为。
            let existing = "- id: ui-settings-models\n  config:\n    credentialOnboarding: true\n";
            fs::write(profile.join(PROFILE_PATCH_FILENAME), existing).expect("写入应当成功");
            ensure_profile(&profile).expect("初始化应当成功");
            assert_eq!(
                fs::read_to_string(profile.join(PROFILE_PATCH_FILENAME)).expect("应当可读"),
                existing
            );
        });
    }

    #[test]
    fn never_overwrites_an_initialized_profile() {
        with_temp_dir(|dir| {
            let profile = dir.join("desktop");
            ensure_profile(&profile).expect("初始化应当成功");
            // 用户改动与 pnpm 安装的记录都不能被下一次启动抹掉。
            let edited = "# 用户自己的补丁层\n";
            fs::write(profile.join(PROFILE_PATCH_FILENAME), edited).expect("写入应当成功");
            fs::write(profile.join("package.json"), "{\"name\":\"custom\"}\n")
                .expect("写入应当成功");
            ensure_profile(&profile).expect("重复初始化应当成功");
            // 清单属于用户，一字不动；补丁层只追加策略，用户内容必须原样保留在前。
            assert_eq!(
                fs::read_to_string(profile.join("package.json")).expect("应当可读"),
                "{\"name\":\"custom\"}\n"
            );
            let patch = fs::read_to_string(profile.join(PROFILE_PATCH_FILENAME)).expect("应当可读");
            assert!(patch.starts_with(edited), "{patch}");
        });
    }
}
