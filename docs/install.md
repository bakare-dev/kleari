# Installing Kleari

## Read me before you install

Kleari runs on a Mac with **Apple silicon** (M1 or later) and **macOS 14 or later**.

**macOS will warn you the first time you open it. That is expected.** Kleari isn't from the
App Store, and it isn't signed with a paid Apple developer account, so macOS can't check it
with Apple. It is signed with Kleari's own certificate instead, which keeps your settings
and the permissions you give it working from one version to the next. You confirm once, in
System Settings, and after that it opens like any other app.

Three things to know before you start:

- **Never choose Move to Trash** in the warning. It throws away the app you just installed.
  Choose **Done** (or **Cancel**, on older macOS) and follow the steps below.
- **Run it from Applications**, not from inside the downloaded disk image.
- **Only install a copy from Kleari's [Releases page](https://github.com/bakare-dev/kleari/releases).**
  The steps below tell macOS to trust this app; don't follow them for a file from anywhere
  else. The repository is private, so sign in to GitHub with an account that has access first.

## Install

### 1. Download and copy it to Applications

1. On the [Releases page](https://github.com/bakare-dev/kleari/releases), open the newest
   release and download **`Kleari_<version>_aarch64.dmg`**.
2. Optional, to check the download is intact: in Terminal, run
   `shasum -a 256 ~/Downloads/Kleari_<version>_aarch64.dmg` and compare the result with the
   checksum in the release notes. They must match exactly.
3. Open the downloaded file. Drag **Kleari** onto the **Applications** folder in the window
   that appears. If an older Kleari is there, choose **Replace** (your history, settings and
   quarantine are kept: they aren't stored in the app).
4. Close the window and eject the disk image (the ⏏ next to **Kleari** in Finder's
   sidebar). You can then delete the downloaded `.dmg`.

### 2. Open it the first time

**macOS 15 (Sequoia) and later:**

1. Open **Applications** and double-click **Kleari**.
2. macOS says *"Kleari" Not Opened* and that Apple could not verify it. Choose **Done**.
   **Not Move to Trash.**
3. Open **System Settings › Privacy & Security**. Scroll down to the **Security** section,
   where it says *"Kleari" was blocked to protect your Mac*, and choose **Open Anyway**.
4. Confirm with your password or Touch ID.
5. macOS asks once more. Choose **Open Anyway**. Kleari opens.

**macOS 14 (Sonoma):**

1. In **Applications**, hold **Control** and click **Kleari** (or right-click it), then
   choose **Open**.
2. macOS warns that it can't verify the developer. Choose **Open**. Kleari opens.

If that warning has no Open button, use the System Settings steps for macOS 15 above; they
work on macOS 14 too.

You do this **once for each version** you install. After that, Kleari opens normally,
including at login if you turn that on.

### 3. If it doesn't open

- **No Open Anyway button in Privacy & Security:** double-click Kleari in Applications
  first (macOS shows the button only after a blocked attempt), then look again. The button
  stays for about an hour after that attempt.
- **"Kleari is damaged and can't be opened"** (or Open Anyway keeps coming back): the copy
  may really be damaged, so don't force it open. Instead:
  1. Move Kleari from Applications to the Trash, download the `.dmg` again from the
     Releases page, and check its checksum as in step 1 above. If it doesn't match, stop and
     don't install it.
  2. Install it again and open it as in step 2.
  3. If macOS still refuses, don't work around it (for example with Terminal commands that
     remove macOS's quarantine): that would skip the check that protects you from a copy that
     isn't genuine. Tell Kleari's maintainer what macOS says instead.

- **You chose Move to Trash by mistake:** open the Trash, drag Kleari back into
  Applications (or download it again), and start from step 2.
- **It opens from the disk image instead:** quit it, make sure it is in Applications, eject the
  disk image, and open it from Applications.
- **"Apple silicon" or macOS version message:** Kleari needs an M1 or later Mac with
  macOS 14 or later; it doesn't run on Intel Macs.

## Permissions Kleari may ask for

Nothing is granted by default, and Kleari works without any of these. It asks only when you
use the feature.

- **Downloads, Desktop, Documents, Movies:** read only when you choose Grant access in
  Cleanup, so a Deep scan can look for large and old files.
- **Full Disk Access** (optional): lets Kleari measure everything on Storage and read
  protected folders without asking for each. Turn it on in System Settings › Privacy &
  Security › Full Disk Access.
- **Notifications:** only if you turn one on in Settings › Notifications, or turn on a
  scheduled scan in Settings › Scanning.
- **Accessibility:** only if you turn on the Mouse page's settings. Kleari then sees your
  mouse wheel and extra buttons (never your keystrokes) to change what they do. Turn it on in
  System Settings › Privacy & Security › Accessibility; the Mouse page offers the way there.
- **Open at login:** only if you turn it on in Settings › General; macOS may ask you to
  approve it in System Settings › General › Login Items.

## The menu bar

Kleari puts an icon in the menu bar. Click it for live readings, your last scan, quarantine
and recent activity; each opens the matching screen. Closing the main window keeps Kleari
running there; quit from the menu bar or with ⌘Q. Turn it off in Settings › General › Show in
menu bar, and closing the window quits again.

## Updating

Kleari checks GitHub for a newer version each time it opens (turn this off in Settings ›
About) and offers it: choose **Install and restart**. You can also choose **Check now** in
Settings › About. An update installs only if it carries Kleari's signature, and your history,
settings and quarantine are kept. The check sends nothing about your Mac or your files.

**Beta updates** (Settings › About) also offers betas: new features early, in releases still
being tested. If you installed a beta, it's on until you choose otherwise, so you keep getting
betas; on a stable release it's off. Turning it off doesn't go back to an older version;
Kleari stays where it is until a newer stable release comes out.

## Uninstalling

1. In Kleari, restore or permanently delete anything still in **Quarantine**. Items there
   are your files, kept until you decide; Kleari won't uninstall until it's empty.
2. Open **Settings › About** and choose **Uninstall…**. Kleari tries to turn off Open at
   login, then moves itself and its settings, history and caches to the Trash, and lists
   what it moved. Your own files aren't touched.
3. To remove the permissions you gave it, run the command it shows in Terminal:
   `tccutil reset All dev.bakare.kleari`. Then choose **Quit Kleari**.

As Kleari quits, macOS may write `~/Library/WebKit/dev.bakare.kleari` once more (it holds
only the light/dark choice); move it to the Trash too if you want nothing left.

To uninstall by hand instead: empty Quarantine, turn off Open at
login, quit Kleari, move it from Applications to the Trash, and move these to the Trash:
`~/Library/Application Support/dev.bakare.kleari` (history, settings and the quarantine
folder), `~/Library/Caches/dev.bakare.kleari`, `~/Library/WebKit/dev.bakare.kleari`,
`~/Library/HTTPStorages/dev.bakare.kleari`,
`~/Library/Saved Application State/dev.bakare.kleari.savedState` and
`~/Library/Preferences/dev.bakare.kleari.plist`.

Kleari never deletes your files on its own, and uninstalling doesn't either.
