#![cfg(target_os = "linux")]
//! LinuxではWebKitGTKの子プロセス用サンドボックスと親プロセスの権限制限を使う。

use std::fs;

pub fn apply_linux_sandbox() -> Result<(), String> {
    // 親のネットワークを切断せず、Webコンテンツの分離をWebKitGTKに任せる。
    if std::env::var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS").as_deref() == Ok("1") {
        return Err("WebKitのサンドボックスを無効にする設定が有効です".into());
    }
    std::env::set_var("WEBKIT_FORCE_SANDBOX", "1");

    // 一般ユーザーでも適用できる制限を使い、失敗した場合は起動を中止する。
    for (option, value) in [(libc::PR_SET_DUMPABLE, 0), (libc::PR_SET_NO_NEW_PRIVS, 1)] {
        if unsafe { libc::prctl(option, value, 0, 0, 0) } != 0 {
            return Err(format!(
                "Linuxの権限制限に失敗しました: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    log::info!("WebKitGTKサンドボックスを強制し、親プロセスの権限昇格とコアダンプを禁止しました");
    Ok(())
}

pub fn get_open_fd_count() -> usize {
    fs::read_dir("/proc/self/fd")
        .map(|entries| entries.count())
        .unwrap_or(0)
}
