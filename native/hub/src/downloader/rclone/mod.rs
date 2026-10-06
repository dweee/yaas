mod cli;
mod files;
mod storage;

pub(super) use cli::{RcloneCli, RcloneTransferOperation, list_remotes};
pub(crate) use files::prepare_rclone_files;
pub(super) use storage::RcloneStorage;
