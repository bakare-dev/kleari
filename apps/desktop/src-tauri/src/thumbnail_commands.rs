//! Pictures for list rows: an image's thumbnail or an app's icon, by the row's ID on its page.
//! The page never sends a path; each page's state says which file an ID stands for.

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::apps::AppsState;
use crate::cleanup::CleanupState;
use crate::cleanup_commands::{blocking_failed, check_ids};
use crate::commands::CommandError;
use crate::files::FilesState;
use crate::storage::explorer::ExplorerState;
use crate::thumbnails::{self, Kind};
use crate::trash::TrashState;

/// Which page's row the picture is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PictureSource {
    /// Files (Large & Old, Duplicates, Similar Images, Downloads, AI clutter).
    Files,
    /// Storage › Browse.
    Explorer,
    /// The Trash tab.
    Trash,
    /// An installed app (its icon).
    App,
    /// A row of the removal sheet: for an app's cache or leftover, its app's icon (when the
    /// Apps page's last scan found the app).
    Planned,
}

/// The picture for row `id` on `source`'s page, at most `size` pixels on its long side, as a
/// `data:` URL; `None` when there is none (not an image, unreadable, or forgotten). An image
/// file gets its thumbnail; an app gets its icon.
#[tauri::command]
pub async fn get_picture(
    app: AppHandle,
    source: PictureSource,
    id: String,
    size: u32,
) -> Result<Option<String>, CommandError> {
    check_ids(std::slice::from_ref(&id))?;
    tauri::async_runtime::spawn_blocking(move || {
        let (path, kind) = match source {
            PictureSource::Files => (app.state::<FilesState>().path(&id), Kind::Image),
            PictureSource::Explorer => (app.state::<ExplorerState>().file_path(&id), Kind::Image),
            PictureSource::Trash => (app.state::<TrashState>().path(&id), Kind::Image),
            PictureSource::App => (
                app.state::<AppsState>()
                    .bundle(&id)
                    .map(|bundle| bundle.path),
                Kind::Icon,
            ),
            PictureSource::Planned => (
                app.state::<CleanupState>()
                    .item(&id)
                    .and_then(|item| app.state::<AppsState>().owner_path(&item.kind, &item.name)),
                Kind::Icon,
            ),
        };
        let Some(path) = path else {
            return Ok(None);
        };
        // Only image files are decoded: any other file never reaches ImageIO.
        if kind == Kind::Image && !crate::files::images::is_image(&path) {
            return Ok(None);
        }
        Ok(thumbnails::picture(&path, kind, size)
            .map(|(bytes, media)| thumbnails::data_url(&bytes, media)))
    })
    .await
    .map_err(blocking_failed)?
}
