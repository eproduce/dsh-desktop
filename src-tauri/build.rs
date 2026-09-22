fn main() {
    // capabilities/ 与 permissions/ 的内容会被 tauri-build 编译进 ACL 清单再嵌入二进制。
    // 如果不声明这两个输入，cargo 在只有它们变化时不会重跑构建脚本，二进制会带着旧清单，
    // 表现为「命令已声明许可却被拒绝」，而且极难排查。
    println!("cargo:rerun-if-changed=capabilities");
    println!("cargo:rerun-if-changed=permissions");
    tauri_build::build()
}
