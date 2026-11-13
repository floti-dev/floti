//! Each OS's layout, the overrides, and that resolving touches nothing.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;
use crate::platform::links::{self, FolderLinks};
use crate::platform::socket::{LocalSocket, Native as NativeSocket};
use crate::testkit::TestDirs;

fn env_with(dirs: &TestDirs, vars: &[(&str, &Path)]) -> UserEnv {
    let mut env = dirs.user_env();
    for (name, value) in vars {
        env.vars
            .insert(OsString::from(name), value.as_os_str().to_owned());
    }
    env
}

fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .expect("read a test folder")
        .next()
        .is_none()
}

#[test]
fn windows_data_and_logs_are_in_localappdata_floti() {
    let dirs = TestDirs::new();
    let local = dirs.folder("Local App Data");
    let folders = windows::Native
        .resolve(&env_with(&dirs, &[("LOCALAPPDATA", &local)]))
        .unwrap();
    assert_eq!(folders.data, local.join("Floti"));
    assert_eq!(folders.logs, local.join("Floti").join("logs"));
    assert_eq!(folders.config, dirs.home.join(".config").join("floti"));
}

#[test]
fn the_config_folder_is_helms_wherever_helms_is_there_and_stays_so() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    assert_eq!(config_folder(&dirs.home), config.join("floti"));
    std::fs::create_dir_all(config.join("helm").join("templates")).unwrap();
    assert_eq!(config_folder(&dirs.home), config.join("helm"));
    for layout in [
        &windows::Native as &dyn FolderLayout,
        &macos::Native,
        &linux::Native,
    ] {
        assert_eq!(
            layout.resolve(&dirs.user_env()).unwrap().config,
            config.join("helm")
        );
    }
    // A ~/.config/floti written meanwhile switches nothing: only the move
    // of Helm's folder does.
    std::fs::create_dir_all(config.join("floti").join("templates")).unwrap();
    assert_eq!(config_folder(&dirs.home), config.join("helm"));
    assert!(
        is_empty(&config.join("floti").join("templates")),
        "resolving made nothing"
    );
}

#[test]
fn windows_without_localappdata_uses_the_profiles_appdata_local() {
    let dirs = TestDirs::new();
    let folders = windows::Native.resolve(&dirs.user_env()).unwrap();
    assert_eq!(
        folders.data,
        dirs.home.join("AppData").join("Local").join("Floti")
    );
}

#[test]
fn macos_data_and_logs_are_in_application_support_floti() {
    let dirs = TestDirs::new();
    let folders = macos::Native.resolve(&dirs.user_env()).unwrap();
    let data = dirs
        .home
        .join("Library")
        .join("Application Support")
        .join("Floti");
    assert_eq!(folders.logs, data.join("logs"));
    assert_eq!(folders.runtime, data.join("run"));
    assert_eq!(folders.data, data);
    assert_eq!(folders.config, dirs.home.join(".config").join("floti"));
}

#[test]
fn linux_defaults_to_local_share_floti_with_logs_in_local_state_floti() {
    let dirs = TestDirs::new();
    let folders = linux::Native.resolve(&dirs.user_env()).unwrap();
    assert_eq!(
        folders.data,
        dirs.home.join(".local").join("share").join("floti")
    );
    assert_eq!(
        folders.logs,
        dirs.home.join(".local").join("state").join("floti")
    );
    assert_eq!(folders.config, dirs.home.join(".config").join("floti"));
    assert_eq!(folders.runtime, folders.data.join("run"));
}

#[test]
fn linux_follows_xdg_data_state_and_runtime_folders() {
    let dirs = TestDirs::new();
    let (data, state, run) = (
        dirs.folder("xdg data"),
        dirs.folder("xdg state"),
        dirs.folder("xdg run"),
    );
    let folders = linux::Native
        .resolve(&env_with(
            &dirs,
            &[
                ("XDG_DATA_HOME", &data),
                ("XDG_STATE_HOME", &state),
                ("XDG_RUNTIME_DIR", &run),
            ],
        ))
        .unwrap();
    assert_eq!(folders.data, data.join("floti"));
    assert_eq!(folders.logs, state.join("floti"));
    assert_eq!(folders.runtime, run.join("floti"));
    assert_eq!(folders.config, dirs.home.join(".config").join("floti"));
}

#[test]
fn linux_ignores_a_relative_xdg_folder() {
    let dirs = TestDirs::new();
    let folders = linux::Native
        .resolve(&env_with(
            &dirs,
            &[("XDG_DATA_HOME", Path::new("relative"))],
        ))
        .unwrap();
    assert_eq!(
        folders.data,
        dirs.home.join(".local").join("share").join("floti")
    );
}

#[test]
fn every_layout_refuses_without_a_home_folder() {
    let env = UserEnv::default();
    let layouts: [&dyn FolderLayout; 3] = [&windows::Native, &macos::Native, &linux::Native];
    for layout in layouts {
        let err = layout.resolve(&env).unwrap_err();
        assert!(err.to_string().contains("home folder"), "{err}");
    }
}

#[test]
fn the_native_layout_is_this_oss() {
    let dirs = TestDirs::new();
    let env = dirs.user_env();
    let expected = match std::env::consts::OS {
        "windows" => windows::Native.resolve(&env),
        "macos" => macos::Native.resolve(&env),
        _ => linux::Native.resolve(&env),
    }
    .unwrap();
    assert_eq!(resolve(&env).unwrap(), expected);
}

#[test]
fn overrides_move_every_folder_and_the_socket_and_touch_nothing_real() {
    let dirs = TestDirs::new();
    let moved = dirs.folder("moved folders");
    let (data, logs, config, run) = (
        moved.join("data"),
        moved.join("logs"),
        moved.join("config"),
        moved.join("run"),
    );
    let env = env_with(
        &dirs,
        &[
            (DATA_DIR_VAR, &data),
            (LOG_DIR_VAR, &logs),
            (CONFIG_DIR_VAR, &config),
            (RUNTIME_DIR_VAR, &run),
        ],
    );

    let folders = resolve(&env).unwrap();
    assert_eq!(
        folders,
        Folders {
            data,
            logs,
            config,
            runtime: run.clone()
        }
    );

    // On macOS and Linux the socket is a file in the runtime folder; on
    // Windows it is a pipe, which has no folder to move.
    let endpoint = NativeSocket.endpoint(&folders.runtime, "host");
    let path = endpoint.as_path();
    assert!(
        path.starts_with(&run) || path.starts_with(r"\\.\pipe\"),
        "the socket is at {}",
        path.display()
    );

    // Resolving creates nothing: neither the moved folders nor anything in
    // the home the layout would have used.
    assert!(is_empty(&moved), "resolving created folders");
    assert!(is_empty(&dirs.home), "resolving touched the home folder");
}

#[test]
fn every_folder_overridden_needs_no_home_folder() {
    let dirs = TestDirs::new();
    let folders = FolderOverrides::under(dirs.root())
        .resolve(&Native, &UserEnv::default())
        .unwrap();
    assert_eq!(folders.data, dirs.root().join("data"));
    assert_eq!(folders.runtime, dirs.root().join("run"));
}

#[test]
fn one_override_moves_only_its_folder() {
    let dirs = TestDirs::new();
    let data = dirs.folder("only data");
    let folders = resolve(&env_with(&dirs, &[(DATA_DIR_VAR, &data)])).unwrap();
    let native = Native.resolve(&dirs.user_env()).unwrap();
    assert_eq!(folders.data, data);
    assert_eq!(folders.logs, native.logs);
    assert_eq!(folders.config, native.config);
    assert_eq!(folders.runtime, native.runtime);
}

#[test]
fn a_relative_override_is_refused_naming_the_variable() {
    let dirs = TestDirs::new();
    let err = resolve(&env_with(&dirs, &[(LOG_DIR_VAR, Path::new("logs here"))])).unwrap_err();
    assert_eq!(
        err.to_string(),
        "FLOTI_LOG_DIR must be an absolute path, and logs here is not."
    );
}

#[test]
fn an_empty_override_is_no_override() {
    let dirs = TestDirs::new();
    let folders = resolve(&env_with(&dirs, &[(DATA_DIR_VAR, &PathBuf::new())])).unwrap();
    assert_eq!(folders, Native.resolve(&dirs.user_env()).unwrap());
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn helms_config_folder_moves_whole_to_flotis_and_is_read_from_there() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    write(&config.join("helm").join("templates").join("a template").join("template.json"), "{}");
    write(&config.join("helm").join("themes").join("mine.json"), "theme");
    write(&config.join("helm").join("agents").join("default.md"), "agent");

    let moved = move_helm_config(&dirs.home);

    assert_eq!(
        moved,
        ConfigMove::Moved {
            from: config.join("helm"),
            to: config.join("floti")
        }
    );
    assert_eq!(moved.config(), Some(config.join("floti").as_path()));
    assert!(!config.join("helm").exists());
    assert_eq!(read(&config.join("floti").join("themes").join("mine.json")), "theme");
    assert_eq!(read(&config.join("floti").join("agents").join("default.md")), "agent");
    assert_eq!(config_folder(&dirs.home), config.join("floti"));
    assert_eq!(move_helm_config(&dirs.home), ConfigMove::Nothing);
    assert!(moved.log_line().unwrap().contains("moved your files"));
}

#[test]
fn helms_files_join_flotis_folder_when_both_are_there_and_none_is_in_both() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    write(&config.join("helm").join("templates").join("t").join("template.json"), "{}");
    write(&config.join("floti").join("agents").join("default.md"), "agent");

    let moved = move_helm_config(&dirs.home);

    assert!(matches!(moved, ConfigMove::Moved { .. }), "{moved:?}");
    assert!(!config.join("helm").exists());
    assert_eq!(read(&config.join("floti").join("templates").join("t").join("template.json")), "{}");
    assert_eq!(read(&config.join("floti").join("agents").join("default.md")), "agent");
}

#[test]
fn nothing_moves_and_nothing_is_replaced_when_an_entry_is_in_both() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    write(&config.join("helm").join("agents").join("default.md"), "helm's");
    write(&config.join("helm").join("themes").join("mine.json"), "theme");
    write(&config.join("floti").join("agents").join("default.md"), "floti's");

    let moved = move_helm_config(&dirs.home);

    let ConfigMove::Kept { reason, .. } = &moved else {
        panic!("{moved:?}");
    };
    assert!(reason.contains("agents"), "{reason}");
    assert_eq!(moved.config(), Some(config.join("helm").as_path()));
    assert_eq!(read(&config.join("helm").join("agents").join("default.md")), "helm's");
    assert_eq!(read(&config.join("floti").join("agents").join("default.md")), "floti's");
    assert!(config.join("helm").join("themes").join("mine.json").is_file());
    assert!(!config.join("floti").join("themes").exists());
}

#[test]
fn a_move_that_fails_partway_puts_back_what_it_moved() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    write(&config.join("helm").join("agents").join("default.md"), "agent");
    write(&config.join("helm").join("themes").join("mine.json"), "theme");
    std::fs::create_dir_all(config.join("floti")).unwrap();

    let moved = move_helm_config_with(&dirs.home, |from, to| {
        if from.ends_with("themes") {
            Err(std::io::Error::other("refused"))
        } else {
            std::fs::rename(from, to)
        }
    });

    assert!(matches!(moved, ConfigMove::Kept { .. }), "{moved:?}");
    assert_eq!(read(&config.join("helm").join("agents").join("default.md")), "agent");
    assert!(config.join("helm").join("themes").join("mine.json").is_file());
    assert!(is_empty(&config.join("floti")));
}

#[test]
fn a_rename_that_fails_leaves_helms_folder_read_where_it_is() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    write(&config.join("helm").join("themes").join("mine.json"), "theme");

    let moved = move_helm_config_with(&dirs.home, |_, _| Err(std::io::Error::other("in use")));

    assert!(matches!(&moved, ConfigMove::Kept { reason, .. } if reason.contains("in use")));
    assert_eq!(moved.config(), Some(config.join("helm").as_path()));
    assert!(config.join("helm").join("themes").join("mine.json").is_file());
    assert!(!config.join("floti").exists());
}

#[test]
fn there_is_nothing_to_move_without_helms_folder() {
    let dirs = TestDirs::new();
    assert_eq!(move_helm_config(&dirs.home), ConfigMove::Nothing);
    assert_eq!(move_helm_config(&dirs.home).config(), None);
    assert!(!dirs.home.join(".config").join("floti").exists());
}

fn folders_with_config(dirs: &TestDirs, config: PathBuf) -> Folders {
    Folders {
        data: dirs.root().join("data"),
        logs: dirs.root().join("logs"),
        config,
        runtime: dirs.root().join("run"),
    }
}

#[test]
fn a_host_that_may_not_write_its_config_folder_moves_nothing() {
    let dirs = TestDirs::new();
    let helm = dirs.home.join(".config").join("helm");
    write(&helm.join("themes").join("mine.json"), "theme");

    let (folders, moved) =
        after_helm_config_move(folders_with_config(&dirs, helm.clone()), Some(&dirs.home), true);

    assert_eq!(moved, ConfigMove::Nothing);
    assert_eq!(folders.config, helm);
    assert!(helm.join("themes").join("mine.json").is_file());
    assert!(!dirs.home.join(".config").join("floti").exists());
}

#[test]
fn a_host_whose_config_folder_was_moved_elsewhere_moves_nothing() {
    let dirs = TestDirs::new();
    let helm = dirs.home.join(".config").join("helm");
    write(&helm.join("themes").join("mine.json"), "theme");
    let elsewhere = dirs.folder("a config folder of its own");

    let (folders, moved) =
        after_helm_config_move(folders_with_config(&dirs, elsewhere.clone()), Some(&dirs.home), false);

    assert_eq!(moved, ConfigMove::Nothing);
    assert_eq!(folders.config, elsewhere);
    assert!(helm.join("themes").join("mine.json").is_file());
    assert!(!dirs.home.join(".config").join("floti").exists());
}

#[test]
fn the_host_that_may_write_helms_config_folder_moves_it_and_reads_flotis() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    write(&config.join("helm").join("themes").join("mine.json"), "theme");

    let (folders, moved) =
        after_helm_config_move(folders_with_config(&dirs, config.join("helm")), Some(&dirs.home), false);

    assert!(matches!(moved, ConfigMove::Moved { .. }), "{moved:?}");
    assert_eq!(folders.config, config.join("floti"));
    assert_eq!(folders.data, dirs.root().join("data"));
    assert_eq!(read(&config.join("floti").join("themes").join("mine.json")), "theme");
    let (_, none) = after_helm_config_move(folders_with_config(&dirs, config.join("floti")), None, false);
    assert_eq!(none, ConfigMove::Nothing);
}

#[test]
fn a_helm_config_folder_that_is_a_link_moves_as_the_link_and_its_target_keeps_everything() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    std::fs::create_dir_all(&config).unwrap();
    let target = dirs.folder("dotfiles helm");
    write(&target.join("themes").join("mine.json"), "theme");
    links::Native.link_folder(&target, &config.join("helm")).unwrap();

    let moved = move_helm_config(&dirs.home);

    assert!(matches!(moved, ConfigMove::Moved { .. }), "{moved:?}");
    assert!(links::is_link(&config.join("floti")), "renamed as the link");
    assert!(!config.join("helm").exists());
    assert_eq!(read(&target.join("themes").join("mine.json")), "theme");
}

#[test]
fn a_helm_config_folder_that_is_a_link_is_never_emptied_into_flotis() {
    let dirs = TestDirs::new();
    let config = dirs.home.join(".config");
    write(&config.join("floti").join("agents").join("default.md"), "agent");
    let target = dirs.folder("dotfiles helm");
    write(&target.join("themes").join("mine.json"), "theme");
    links::Native.link_folder(&target, &config.join("helm")).unwrap();

    let moved = move_helm_config(&dirs.home);

    let ConfigMove::Kept { reason, .. } = &moved else {
        panic!("{moved:?}");
    };
    assert!(reason.contains("is a link"), "{reason}");
    assert!(links::is_link(&config.join("helm")));
    assert_eq!(read(&target.join("themes").join("mine.json")), "theme");
    assert!(!config.join("floti").join("themes").exists());
}
