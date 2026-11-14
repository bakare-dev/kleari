//! Browser care (ADR-022): your open tabs in Safari and Chrome-family browsers, so you can close
//! the ones you choose. Read and closed through each browser's own AppleScript, which macOS
//! allows only after you agree in its Automation prompt.
//!
//! Each script is fixed per browser; a tab to close is passed as arguments (window, tab and the
//! address it showed), never written into the script, and is closed only if it still shows that
//! address. Every script first checks the browser is running, so asking never opens it. Tab
//! addresses stay in Rust; the page gets each tab's title and site name.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const OSASCRIPT: &str = "/usr/bin/osascript";
const TIME_LIMIT: Duration = Duration::from_secs(30);
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;
const MAX_TABS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Browser {
    Safari,
    Chrome,
    Brave,
    Edge,
}

impl Browser {
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Safari, Self::Chrome, Self::Brave, Self::Edge];

    /// The application name AppleScript addresses: a constant, never data.
    pub const fn app_name(self) -> &'static str {
        match self {
            Self::Safari => "Safari",
            Self::Chrome => "Google Chrome",
            Self::Brave => "Brave Browser",
            Self::Edge => "Microsoft Edge",
        }
    }

    pub const fn bundle_id(self) -> &'static str {
        match self {
            Self::Safari => "com.apple.Safari",
            Self::Chrome => "com.google.Chrome",
            Self::Brave => "com.brave.Browser",
            Self::Edge => "com.microsoft.edgemac",
        }
    }

    /// Safari names a tab's title `name`; Chrome-family browsers `title`.
    const fn title_property(self) -> &'static str {
        match self {
            Self::Safari => "name",
            _ => "title",
        }
    }

    /// Inside `tell application`, `tab` names the browser's tab class, not the tab character,
    /// so the separator is made before it. Titles and addresses are read one window at a time
    /// (two Apple events per window, not two per tab); a window without tabs is skipped.
    fn list_script(self) -> String {
        format!(
            r#"set sep to character id 9
set out to ""
if application "{app}" is running then
  tell application "{app}"
    set wi to 0
    repeat with w in windows
      set wi to wi + 1
      try
        set titles to {title} of every tab of w
        set addresses to URL of every tab of w
        repeat with ti from 1 to count of titles
          set t to item ti of titles
          if t is missing value then set t to ""
          set a to item ti of addresses
          if a is missing value then set a to ""
          set out to out & wi & sep & ti & sep & t & sep & a & linefeed
        end repeat
      end try
    end repeat
  end tell
end if
return out"#,
            app = self.app_name(),
            title = self.title_property(),
        )
    }

    fn close_script(self) -> String {
        format!(
            r#"on run argv
  set wi to (item 1 of argv) as integer
  set ti to (item 2 of argv) as integer
  set address to item 3 of argv
  if application "{app}" is running then
    tell application "{app}"
      try
        set t to tab ti of window wi
        if (URL of t) is address then
          close t
          return "closed"
        end if
      end try
    end tell
  end if
  return "changed"
end run"#,
            app = self.app_name(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTab {
    pub id: String,
    pub window: u32,
    pub title: String,
    /// The site's host name (`example.com`); the full address stays in Rust.
    pub site: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ListStatus {
    Listed,
    NotRunning,
    /// macOS's Automation permission for this browser is off.
    NotAllowed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabList {
    pub browser: Browser,
    pub status: ListStatus,
    pub tabs: Vec<BrowserTab>,
}

#[derive(Debug, Clone)]
struct Kept {
    browser: Browser,
    window: u32,
    tab: u32,
    address: String,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The tabs of the last listing of each browser, by ID.
#[derive(Default)]
pub struct BrowserState {
    tabs: Mutex<HashMap<String, Kept>>,
}

enum Ran {
    Output(String),
    NotAllowed,
    Failed,
}

fn osascript(script: &str, args: &[String]) -> Ran {
    let Ok(mut child) = Command::new(OSASCRIPT)
        .arg("-e")
        .arg(script)
        .args(args)
        .env_clear()
        .env("LC_ALL", "en_US.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return Ran::Failed;
    };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Ran::Failed;
    };
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(MAX_OUTPUT).read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.take(64 * 1024).read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < TIME_LIMIT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let output = out.join().unwrap_or_default();
    let errors = err.join().unwrap_or_default();
    match status {
        Some(status) if status.success() => Ran::Output(output),
        // errAEEventNotPermitted: the Automation permission is off.
        Some(_) if errors.contains("-1743") => Ran::NotAllowed,
        _ => Ran::Failed,
    }
}

/// The host part of an address, for display.
fn site(address: &str) -> String {
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    let host = host.split(':').next().unwrap_or_default();
    if host.is_empty() {
        address.split(':').next().unwrap_or_default().to_owned()
    } else {
        host.trim_start_matches("www.").to_owned()
    }
}

/// Lines of `window⇥tab⇥title⇥address`; malformed lines are skipped.
pub fn parse(output: &str) -> Vec<(u32, u32, String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let window = parts.next()?.trim().parse().ok()?;
            let tab = parts.next()?.trim().parse().ok()?;
            let title = parts.next()?.chars().take(300).collect::<String>();
            let address = parts.next()?.to_owned();
            Some((window, tab, title, address))
        })
        .take(MAX_TABS)
        .collect()
}

impl BrowserState {
    /// Lists `browser`'s tabs now. The first time, macOS asks whether Kleari may control it.
    pub fn list(&self, browser: Browser, running: bool) -> TabList {
        let mut list = TabList {
            browser,
            status: ListStatus::Listed,
            tabs: Vec::new(),
        };
        if !running {
            list.status = ListStatus::NotRunning;
            return list;
        }
        let output = match osascript(&browser.list_script(), &[]) {
            Ran::Output(output) => output,
            Ran::NotAllowed => {
                list.status = ListStatus::NotAllowed;
                return list;
            }
            Ran::Failed => {
                list.status = ListStatus::Failed;
                return list;
            }
        };
        let mut kept = lock(&self.tabs);
        kept.retain(|_, tab| tab.browser != browser);
        for (window, tab, title, address) in parse(&output) {
            let id = crate::ids::new_id(0);
            list.tabs.push(BrowserTab {
                id: id.clone(),
                window,
                title: if title.trim().is_empty() {
                    site(&address)
                } else {
                    title
                },
                site: site(&address),
            });
            kept.insert(
                id,
                Kept {
                    browser,
                    window,
                    tab,
                    address,
                },
            );
        }
        list
    }

    /// Closes the chosen tabs, each only if it still shows the address it was listed with.
    /// Within a window, later tabs close first so the earlier ones keep their places. Returns
    /// how many closed, and whether the Automation permission was off.
    pub fn close(&self, ids: &[String]) -> (u32, bool) {
        let mut chosen: Vec<(String, Kept)> = {
            let kept = lock(&self.tabs);
            ids.iter()
                .filter_map(|id| kept.get(id).map(|tab| (id.clone(), tab.clone())))
                .collect()
        };
        chosen.sort_by_key(|(_, tab)| {
            (
                tab.browser.app_name(),
                tab.window,
                std::cmp::Reverse(tab.tab),
            )
        });
        let mut closed = 0;
        let mut not_allowed = false;
        for (id, tab) in chosen {
            let args = [
                tab.window.to_string(),
                tab.tab.to_string(),
                tab.address.clone(),
            ];
            match osascript(&tab.browser.close_script(), &args) {
                Ran::Output(output) if output.trim() == "closed" => {
                    closed += 1;
                    lock(&self.tabs).remove(&id);
                }
                Ran::NotAllowed => not_allowed = true,
                _ => {}
            }
        }
        (closed, not_allowed)
    }
}

/// Lists a browser's tabs (macOS asks the first time whether Kleari may control it).
#[tauri::command]
pub async fn list_browser_tabs(
    app: tauri::AppHandle,
    browser: Browser,
) -> Result<TabList, crate::commands::CommandError> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let running = crate::system_monitor::running_apps::running_bundles(&app)
            .map(|apps| apps.iter().any(|running| running.id == browser.bundle_id()))
            // Unknown: let the script, which never opens the browser, find out.
            .unwrap_or(true);
        app.state::<BrowserState>().list(browser, running)
    })
    .await
    .map_err(crate::cleanup_commands::blocking_failed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Closed {
    pub closed: u32,
    pub not_allowed: bool,
}

/// Closes the chosen tabs (the page asked you first).
#[tauri::command]
pub async fn close_browser_tabs(
    app: tauri::AppHandle,
    ids: Vec<String>,
) -> Result<Closed, crate::commands::CommandError> {
    use tauri::Manager;
    crate::cleanup_commands::check_ids(&ids)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (closed, not_allowed) = app.state::<BrowserState>().close(&ids);
        Closed {
            closed,
            not_allowed,
        }
    })
    .await
    .map_err(crate::cleanup_commands::blocking_failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listings_parse_and_odd_lines_are_skipped() {
        let output = "1\t1\tInbox\thttps://mail.example.com/u/0\n1\t2\t\thttps://www.example.org/a?b\nbroken line\n2\t1\tTitle\twith\ttabs\tfile:///Users/x\n";
        let tabs = parse(output);
        assert_eq!(tabs.len(), 3);
        assert_eq!(
            tabs[0],
            (
                1,
                1,
                "Inbox".to_owned(),
                "https://mail.example.com/u/0".to_owned()
            )
        );
        assert_eq!(site(&tabs[1].3), "example.org");
        assert_eq!(site("https://user@host.example:8080/x"), "host.example");
        assert_eq!(site("about:blank"), "about");
    }

    #[test]
    fn scripts_check_the_browser_runs_and_take_the_tab_as_arguments() {
        for browser in Browser::ALL {
            let list = browser.list_script();
            let close = browser.close_script();
            let running = format!("if application \"{}\" is running then", browser.app_name());
            assert!(list.contains(&running) && close.contains(&running));
            assert!(close.contains("item 3 of argv") && close.contains("if (URL of t) is address"));
        }
    }

    #[test]
    fn the_listing_never_uses_tab_as_its_separator_inside_the_browser() {
        // In a browser's dictionary `tab` is the tab class: joining with it failed every
        // window, and every listing came back empty.
        for browser in Browser::ALL {
            let list = browser.list_script();
            let (before, inside) = list.split_once("tell application").expect("tell block");
            assert!(before.contains("set sep to character id 9"));
            assert!(!inside.contains("& tab &"));
            assert!(inside.contains("& sep &"));
        }
    }
}
