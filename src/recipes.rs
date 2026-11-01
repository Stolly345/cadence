//! The built-in examples: ready-made jobs in plain words. Each asks at most a couple of simple
//! questions (a folder, a drive) and fills in everything else.

use crate::jobs::{Job, Kind};

#[derive(Clone, Copy)]
pub enum Ask {
    /// Pick a folder; the answer goes in this param.
    Folder { key: &'static str, label: &'static str },
    /// Pick one or more folders (answer: one per line) for `src`.
    Folders { label: &'static str },
    /// Pick a plugged-in drive. The backup goes to <drive>/Cadence Backups/<sub>, and the job
    /// skips its run while that drive isn't plugged in.
    Drive { label: &'static str, sub: &'static str },
    /// Type something; the answer goes in this param.
    Text { key: &'static str, label: &'static str, example: &'static str },
    /// A password (shown as dots); goes in `password`. May be left blank.
    Password { label: &'static str },
}

pub struct Recipe {
    pub category: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    pub icon: &'static str,
    pub kind: Kind,
    pub params: &'static [(&'static str, &'static str)],
    pub schedule: &'static str,
    pub asks: &'static [Ask],
    /// Needs the admin password (runs as root).
    pub root: bool,
}

pub const CATEGORIES: &[&str] = &["Back up", "Tidy up", "Keep healthy", "Handy tools"];

const SRC: Ask = Ask::Folders { label: "Folders to back up" };
const DRIVE: Ask = Ask::Drive { label: "Back up to", sub: "" };

pub const RECIPES: &[Recipe] = &[
    Recipe {
        category: "Back up", title: "Back up folders to a USB drive", icon: "drive-removable-media-symbolic",
        blurb: "Copies folders (like Documents and Pictures) to a drive. Only new and changed files are copied, so after the first time it's quick. If the drive isn't plugged in, it waits for next time.",
        kind: Kind::Rsync, params: &[("src", "~/Documents")], schedule: "0 20 * * *", asks: &[SRC, DRIVE], root: false,
    },
    Recipe {
        category: "Back up", title: "Back up folders to a NAS", icon: "network-server-symbolic",
        blurb: "Copies folders to a network storage box (Synology, QNAP, WD My Cloud, TrueNAS…) over your home network. Turn on the NAS's rsync service first (Synology: Control Panel → File Services → rsync). When you're away from home, it waits for next time.",
        kind: Kind::Rsync, params: &[("src", "~/Documents")], schedule: "0 20 * * *",
        asks: &[SRC,
            Ask::Text { key: "nas_host", label: "NAS address", example: "192.168.1.50 or mynas.local" },
            Ask::Text { key: "nas_path", label: "Shared folder on the NAS, and a folder inside it", example: "backups/laptop" },
            Ask::Text { key: "nas_user", label: "NAS user name", example: "me" },
            Ask::Password { label: "NAS password" }],
        root: false,
    },
    Recipe {
        category: "Back up", title: "Keep old versions of my files", icon: "document-open-recent-symbolic",
        blurb: "Like the backup above, but keeps a separate copy for each day, week and month, so you can get back a file from before you changed or deleted it. Unchanged files don't take extra space.",
        kind: Kind::Snapshot, params: &[("src", "~/Documents")], schedule: "0 20 * * *",
        asks: &[SRC, Ask::Drive { label: "Back up to", sub: "Snapshots" }], root: false,
    },
    Recipe {
        category: "Back up", title: "Back up my settings", icon: "preferences-system-symbolic",
        blurb: "Copies your app settings (the hidden .config folder) to a drive, leaving out caches. Private keys (.ssh, .gnupg) aren't included: add them only if the drive is encrypted.",
        kind: Kind::Rsync, params: &[("src", "~/.config"), ("exclude", SETTINGS_EXCLUDE)], schedule: "0 20 * * *",
        asks: &[Ask::Folders { label: "Settings folders to back up" }, Ask::Drive { label: "Back up to", sub: "Settings" }],
        root: false,
    },
    Recipe {
        category: "Back up", title: "Check my USB backup is complete", icon: "object-select-symbolic",
        blurb: "Compares folders with the copies made by “Back up folders to a USB drive” and tells you if anything is missing or different.",
        kind: Kind::Verify, params: &[("src", "~/Documents"), ("checksum", "yes")], schedule: "0 12 * * 0",
        asks: &[Ask::Folders { label: "Folders that are backed up" }, Ask::Drive { label: "Backup drive", sub: "" }], root: false,
    },
    Recipe {
        category: "Back up", title: "Copy folders to another computer", icon: "computer-symbolic",
        blurb: "Copies folders to another computer over SSH. For people who already sign in to that computer with an SSH key (no password prompt).",
        kind: Kind::Rsync, params: &[("src", "~/Documents")], schedule: "0 20 * * *",
        asks: &[SRC, Ask::Text { key: "dest", label: "Other computer and folder", example: "me@192.168.1.20:/home/me/Backups" }], root: false,
    },
    Recipe {
        category: "Back up", title: "Pull files from another computer", icon: "network-transmit-receive-symbolic",
        blurb: "The other way round: copies a folder from another computer or a NAS onto this one, such as a home server's backups. For people who sign in to it with an SSH key, or use the NAS's rsync service.",
        kind: Kind::Pull, params: &[("dest", "~/Backups/server")], schedule: "0 20 * * *",
        asks: &[Ask::Text { key: "from", label: "Copy from", example: "me@192.168.1.50:/var/backups or rsync://me@nas/share/folder" },
            Ask::Folder { key: "dest", label: "Into this folder" }, Ask::Password { label: "NAS password (rsync:// only)" }],
        root: false,
    },
    Recipe {
        category: "Tidy up", title: "Sort my Downloads folder", icon: "folder-download-symbolic",
        blurb: "Moves downloads into Images, Documents, Music, Videos, Archives and Installers folders. Leaves anything downloaded in the last hour, and never overwrites a file.",
        kind: Kind::SortDownloads, params: &[], schedule: "*/30 * * * *", asks: &[], root: false,
    },
    Recipe {
        category: "Tidy up", title: "Empty old things from the trash", icon: "user-trash-symbolic",
        blurb: "Permanently deletes things that have been in the trash for more than 30 days.",
        kind: Kind::DeleteOld, params: &[("dir", "~/.local/share/Trash"), ("days", "30")], schedule: "0 12 * * 0", asks: &[], root: false,
    },
    Recipe {
        category: "Tidy up", title: "Clear old cache files", icon: "edit-clear-all-symbolic",
        blurb: "Apps keep temporary files in a cache folder that only grows. This deletes ones not touched for 30 days; apps recreate anything they still need.",
        kind: Kind::DeleteOld, params: &[("dir", "~/.cache"), ("days", "30")], schedule: "0 12 1 * *", asks: &[], root: false,
    },
    Recipe {
        category: "Tidy up", title: "Delete old files from a folder", icon: "edit-delete-symbolic",
        blurb: "Permanently deletes files older than 90 days from one folder, such as a folder of screenshots. Choose carefully: deleted files don't go to the trash.",
        kind: Kind::DeleteOld, params: &[("days", "90")], schedule: "0 12 * * 0",
        asks: &[Ask::Folder { key: "dir", label: "Folder to clean" }, Ask::Text { key: "days", label: "Delete files older than this many days", example: "90" }], root: false,
    },
    Recipe {
        category: "Tidy up", title: "Free up space from old system files", icon: "package-x-generic-symbolic",
        blurb: "Removes system packages nothing needs any more, update files kept from earlier updates, unused Flatpak runtimes, and system logs older than two weeks. Needs your password.",
        kind: Kind::Script,
        params: &[("body", FREE_SPACE)],
        schedule: "0 12 1 * *", asks: &[], root: true,
    },
    Recipe {
        category: "Keep healthy", title: "Keep my computer updated", icon: "software-update-available-symbolic",
        blurb: "Installs system updates every day, and tells you when a restart is needed to finish. Needs your password once, when you turn it on.",
        kind: Kind::Updates, params: &[("upgrade", "yes")], schedule: "0 12 * * *", asks: &[], root: true,
    },
    Recipe {
        category: "Keep healthy", title: "Tell me when updates are available", icon: "software-update-urgent-symbolic",
        blurb: "Checks for updates every day and lets you know, without installing anything.",
        kind: Kind::Updates, params: &[("upgrade", "no")], schedule: "0 12 * * *", asks: &[], root: true,
    },
    Recipe {
        category: "Keep healthy", title: "Check my drives' health", icon: "drive-harddisk-solidstate-symbolic",
        blurb: "Asks each drive whether it's wearing out or failing (its built-in SMART check) and warns you in time to save your files. Needs your password, and the smartmontools package.",
        kind: Kind::Script, params: &[("body", SMART)], schedule: "0 12 * * 0", asks: &[], root: true,
    },
    Recipe {
        category: "Keep healthy", title: "Check my laptop battery", icon: "battery-good-symbolic",
        blurb: "Checks how much charge the battery can still hold compared with when it was new, and tells you when it drops below 70%.",
        kind: Kind::Script, params: &[("body", BATTERY)], schedule: "0 12 * * 0", asks: &[], root: false,
    },
    Recipe {
        category: "Keep healthy", title: "Warn me when my disk is nearly full", icon: "drive-harddisk-symbolic",
        blurb: "Checks every hour and shows a warning when the disk is 80% full, and an urgent one at 90%.",
        kind: Kind::DiskAlert, params: &[], schedule: "0 * * * *", asks: &[], root: false,
    },
    Recipe {
        category: "Handy tools", title: "What's using my disk space?", icon: "drive-harddisk-symbolic",
        blurb: "Lists the biggest folders in your home folder, largest first.",
        kind: Kind::Script,
        params: &[("body", "echo \"Biggest folders in $HOME:\"\ndu -xh --max-depth=1 \"$HOME\" 2>/dev/null | sort -rh | head -n 20\n")],
        schedule: "manual", asks: &[], root: false,
    },
    Recipe {
        category: "Handy tools", title: "Find my biggest files", icon: "edit-find-symbolic",
        blurb: "Lists the 25 biggest files in your home folder.",
        kind: Kind::Script,
        params: &[("body", "find \"$HOME\" -xdev -type f -size +50M -printf '%s\\t%p\\n' 2>/dev/null | sort -rn | head -n 25 | numfmt --to=iec --field=1\n")],
        schedule: "manual", asks: &[], root: false,
    },
    Recipe {
        category: "Handy tools", title: "Show my computer's details", icon: "computer-symbolic",
        blurb: "Shows the system version, how long it's been on, memory, disks and drives.",
        kind: Kind::Script,
        params: &[("body", "echo \"== System ==\"; cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d= -f2 | tr -d '\"'\nuname -r\necho; echo \"== On for ==\"; uptime -p\necho; echo \"== Memory ==\"; free -h\necho; echo \"== Disks ==\"; df -h -x tmpfs -x devtmpfs -x squashfs -x efivarfs\necho; echo \"== Drives ==\"; lsblk -o NAME,SIZE,TYPE,MOUNTPOINTS -e 7\n")],
        schedule: "manual", asks: &[], root: false,
    },
    Recipe {
        category: "Handy tools", title: "What's slowing my computer down?", icon: "power-profile-performance-symbolic",
        blurb: "Lists the programs using the most processor and the most memory right now.",
        kind: Kind::Script,
        params: &[("body", "echo \"== Most processor ==\"\nps -eo pid,comm,%cpu,%mem --sort=-%cpu | head -n 11\necho; echo \"== Most memory ==\"\nps -eo pid,comm,%mem,rss --sort=-%mem | head -n 11\n")],
        schedule: "manual", asks: &[], root: false,
    },
    Recipe {
        category: "Handy tools", title: "Check my internet connection", icon: "network-wireless-symbolic",
        blurb: "Tests each step in turn (your router, the internet, website names) and says which one fails.",
        kind: Kind::Script,
        params: &[("body", "gw=$(ip route | awk '/^default/ {print $3; exit}')\nif [ -z \"$gw\" ]; then echo \"✗ Not connected to any network\"; exit 1; fi\nif ping -c 2 -W 2 \"$gw\" >/dev/null; then echo \"✓ Router ($gw) answers\"; else echo \"✗ Router ($gw) doesn't answer\"; exit 1; fi\nif ping -c 2 -W 3 1.1.1.1 >/dev/null; then echo \"✓ The internet is reachable\"; else echo \"✗ The internet isn't reachable (your router or provider)\"; exit 1; fi\nif getent hosts example.com >/dev/null; then echo \"✓ Website names work (DNS)\"; else echo \"✗ Website names don't work (DNS problem)\"; exit 1; fi\necho \"All good.\"\n")],
        schedule: "manual", asks: &[], root: false,
    },
    Recipe {
        category: "Handy tools", title: "Show my network addresses", icon: "network-workgroup-symbolic",
        blurb: "Shows this computer's address on your home network, for connecting from another device.",
        kind: Kind::Script,
        params: &[("body", "echo \"This computer: $(hostname)\"\nip -brief -4 addr show scope global\n")],
        schedule: "manual", asks: &[], root: false,
    },
    Recipe {
        category: "Handy tools", title: "Sync my notes with git", icon: "emblem-synchronizing-symbolic",
        blurb: "Saves changes in a notes folder (Obsidian, Logseq, plain text…) to git and syncs with your other computers through its remote (GitHub, GitLab…). The folder must already be a git repository with a remote.",
        kind: Kind::Script, params: &[("body", GIT_SYNC), ("notes", "~/Notes")], schedule: "0 * * * *",
        asks: &[Ask::Folder { key: "notes", label: "Notes folder" }], root: false,
    },
    Recipe {
        category: "Handy tools", title: "What's listening on my network?", icon: "network-wired-symbolic",
        blurb: "Lists the programs on this computer that accept connections, and whether only this computer or your whole network can reach them.",
        kind: Kind::Script, params: &[("body", LISTENING)], schedule: "manual", asks: &[], root: false,
    },
];

const FREE_SPACE: &str = r#"before=$(df --output=avail -B1 / | tail -n 1)
if command -v apt-get >/dev/null; then apt-get -y autoremove; apt-get clean
elif command -v dnf >/dev/null; then dnf -y autoremove; dnf clean packages
elif command -v zypper >/dev/null; then zypper -n clean --all
fi
if command -v flatpak >/dev/null; then flatpak uninstall --unused -y --noninteractive; fi
if command -v journalctl >/dev/null; then journalctl --vacuum-time=14d; fi
after=$(df --output=avail -B1 / | tail -n 1)
freed=$(( (after - before) / 1048576 ))
if [ "$freed" -gt 0 ]; then echo "Freed about ${freed} MB"; else echo "Nothing much to free"; fi
df -h /
"#;

// Caches and lock files that apps rebuild themselves: no point backing them up.
const SETTINGS_EXCLUDE: &str = "Cache\nCache_Data\nCode Cache\nGPUCache\nDawnCache\nGrShaderCache\nShaderCache\nCrashpad\nCrash Reports\n*.sock\nSingleton*\n*.lock\n";

// smartctl's exit code is a bitmask: 2 = couldn't open the drive or no SMART (common behind
// USB enclosures: reported, never alarmed), 8 = drive says it is failing, 16 = a wear
// attribute is past its threshold now. Only 8 and 16 alarm.
const SMART: &str = r#"command -v smartctl >/dev/null || fail "Install the smartmontools package first, then run this again"
bad=0
for dev in $(lsblk -dn -o NAME,TYPE | awk '$2=="disk" && $1 !~ /^(zram|loop|ram)/ {print "/dev/"$1}'); do
  smartctl -H "$dev" >/dev/null 2>&1; rc=$?
  if (( rc & 2 )); then echo "– $dev: can't read its health (normal for many USB drives)"; continue; fi
  if (( rc & 24 )); then echo "✗ $dev is reporting that it is failing: back up its files now"; bad=1
  else echo "✓ $dev: healthy"; fi
  temp=$(smartctl -j -A "$dev" 2>/dev/null | tr -d ' \n' | grep -o '"temperature":{"current":[0-9]*' | grep -o '[0-9]*$')
  if [ -n "$temp" ]; then
    echo "  temperature ${temp}°C"
    if [ "$temp" -ge 65 ]; then notify normal "$dev is running hot (${temp}°C)"; fi
  fi
done
exit $bad
"#;

const BATTERY: &str = r#"found=0
for b in /sys/class/power_supply/BAT*; do
  [ -d "$b" ] || continue
  full=$(cat "$b/energy_full" 2>/dev/null || cat "$b/charge_full" 2>/dev/null)
  design=$(cat "$b/energy_full_design" 2>/dev/null || cat "$b/charge_full_design" 2>/dev/null)
  [ -n "$full" ] && [ -n "$design" ] && [ "$design" -gt 0 ] || continue
  found=1
  pct=$(( full * 100 / design ))
  cycles=$(cat "$b/cycle_count" 2>/dev/null || echo "unknown")
  echo "${b##*/}: holds ${pct}% of the charge it held when new (charge cycles: $cycles)"
  if [ "$pct" -lt 70 ]; then notify normal "The battery holds only ${pct}% of its original charge: it may be time to replace it"; fi
done
if [ $found = 0 ]; then echo "No battery found: this looks like a desktop computer."; fi
"#;

// Commits local changes, rebases onto the remote, pushes. A remote that can't be reached is a
// skip (offline); a merge conflict is a failure, left for a person to sort out.
const GIT_SYNC: &str = r#"cd "$NOTES" 2>/dev/null || fail "$NOTES not found"
git rev-parse --git-dir >/dev/null 2>&1 || fail "$NOTES isn't a git repository yet (git init, then add a remote)"
export GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND="ssh -o BatchMode=yes -o ConnectTimeout=15"
git ls-remote -q origin >/dev/null 2>&1 || skip "can't reach the git remote (offline?)"
who=()
git config user.name >/dev/null || who+=(-c "user.name=$(id -un)")
git config user.email >/dev/null || who+=(-c "user.email=$(id -un)@$(hostname)")
git add -A
if ! git diff --cached --quiet; then git "${who[@]}" commit -q -m "Sync from $(hostname), $(date '+%F %R')" || fail "couldn't commit"; fi
branch=$(git symbolic-ref --short HEAD)
# a brand-new remote has nothing to pull yet
if git ls-remote --exit-code -q --heads origin "$branch" >/dev/null 2>&1; then
  if ! git "${who[@]}" pull -q --rebase origin "$branch"; then
    git rebase --abort 2>/dev/null
    fail "Changes made elsewhere clash with yours: open $NOTES and merge them by hand"
  fi
fi
git push -q -u origin "$branch" || fail "couldn't push to the remote"
echo "Synced: $(git log -1 --format='%h %s')"
"#;

const LISTENING: &str = r#"echo "Programs on this computer that accept connections:"
echo
ss -H -tulpn 2>/dev/null | awk '{
  local=$5; n=split(local, a, ":"); port=a[n]; addr=substr(local, 1, length(local)-length(port)-1)
  prog=$7; gsub(/users:\(\("/, "", prog); gsub(/".*/, "", prog); if (prog == "") prog="(a system service)"
  who=(addr ~ /^(127\.|\[::1\]|::1)/ || addr ~ /%lo$/) ? "this computer only" : "your network"
  printf "%-5s port %-6s %-20s %s\n", $1, port, who, prog
}' | sort -u -k3,3n
echo
echo "\"your network\" means other devices on your Wi-Fi can connect; a firewall can still block them."
"#;

impl Recipe {
    /// Turn the answers (one per ask, in order) and a schedule into a ready job.
    pub fn job(&self, answers: &[String], schedule: &str) -> Result<Job, String> {
        let mut j = Job::new(self.kind);
        j.name = self.title.into();
        j.schedule = schedule.into();
        j.root = self.kind.forced_root().unwrap_or(self.root);
        j.notify_success = false;
        for (k, v) in self.params { j.params.insert(k.to_string(), v.to_string()); }
        for (a, v) in self.asks.iter().zip(answers) {
            let v = v.trim();
            match a {
                Ask::Folder { key, label } | Ask::Text { key, label, .. } => {
                    if v.is_empty() { return Err(format!("{label}: please fill this in")); }
                    j.params.insert(key.to_string(), v.into());
                }
                Ask::Folders { label } => {
                    if v.is_empty() { return Err(format!("{label}: add at least one folder")); }
                    j.params.insert("src".into(), v.into());
                }
                Ask::Password { .. } => { j.params.insert("password".into(), v.into()); }
                Ask::Drive { label, sub } => {
                    if v.is_empty() { return Err(format!("{label}: plug in a drive and pick it")); }
                    let root = v.trim_end_matches('/');
                    let dest = if sub.is_empty() { format!("{root}/Cadence Backups") } else { format!("{root}/Cadence Backups/{sub}") };
                    j.params.insert("dest".into(), dest);
                    // a picked drive must be mounted to run; an "Other folder…" pick needn't be
                    if drives().iter().any(|(_, m)| m == root) || root.starts_with("/media/") || root.starts_with("/run/media/") {
                        j.params.insert("require_mount".into(), root.into());
                    }
                }
            }
        }
        // NAS: rsync://user@host/share/path from the separate answers
        if let Some(host) = j.params.remove("nas_host") {
            let path = j.params.remove("nas_path").unwrap_or_default();
            let user = j.params.remove("nas_user").unwrap_or_default();
            let host = host.trim().trim_start_matches("rsync://").trim_end_matches('/');
            let user = if user.trim().is_empty() { String::new() } else { format!("{}@", user.trim()) };
            j.params.insert("dest".into(), format!("rsync://{user}{host}/{}", path.trim().trim_matches('/')));
        }
        crate::jobs::validate(&j)?;
        Ok(j)
    }
}

/// Plugged-in drives: (name to show, mount point).
pub fn drives() -> Vec<(String, String)> {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut v: Vec<(String, String)> = mounts.lines().filter_map(|l| l.split(' ').nth(1)).map(unescape)
        .filter(|m| ["/media/", "/run/media/", "/mnt/"].iter().any(|p| m.starts_with(p)))
        .map(|m| (m.rsplit('/').next().unwrap_or(&m).to_string(), m))
        .collect();
    v.dedup_by(|a, b| a.1 == b.1);
    v
}

/// /proc/mounts writes spaces etc. as \040-style octal escapes.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 4 <= b.len() && let Some(n) = std::str::from_utf8(&b[i + 1..i + 4]).ok().and_then(|o| u8::from_str_radix(o, 8).ok()) { out.push(n); i += 4; } else { out.push(b[i]); i += 1; }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recipe_makes_a_valid_job_and_valid_bash() {
        let ctx = crate::jobs::Ctx { user: "me".into(), uid: 1000, home: "/home/me".into() };
        for r in RECIPES {
            assert!(CATEGORIES.contains(&r.category), "{}", r.title);
            let answers: Vec<String> = r.asks.iter().map(|a| match a {
                Ask::Text { example, .. } => example.split(' ').next().unwrap().to_string(),
                Ask::Password { .. } => "secret".into(),
                Ask::Folders { .. } => "/home/me/Documents\n/home/me/Pictures".into(),
                _ => "/media/me/USB STICK".into(),
            }).collect();
            if r.asks.iter().any(|a| !matches!(a, Ask::Password { .. })) { assert!(r.job(&vec![String::new(); r.asks.len()], r.schedule).is_err(), "{} accepts blanks", r.title); }
            let j = r.job(&answers, r.schedule).unwrap_or_else(|e| panic!("{}: {e}", r.title));
            let out = std::process::Command::new("bash").args(["-n", "-c", &ctx.script(&j)]).output().unwrap();
            assert!(out.status.success(), "{}: {}", r.title, String::from_utf8_lossy(&out.stderr));
        }
    }

    #[test]
    fn drive_backups_land_in_a_named_folder_and_need_the_drive() {
        let r = &RECIPES[0];
        let j = r.job(&["/home/me/Pictures\n/home/me/Documents".into(), "/media/me/USB STICK".into()], r.schedule).unwrap();
        assert_eq!(j.params["dest"], "/media/me/USB STICK/Cadence Backups");
        assert_eq!(j.params["require_mount"], "/media/me/USB STICK");
        assert!(r.job(&["/home/me/a/Docs\n/home/me/b/Docs".into(), "/media/me/X".into()], r.schedule).is_err(), "same-name folders");
    }

    #[test]
    fn nas_answers_make_an_rsync_url() {
        let r = RECIPES.iter().find(|r| r.title.contains("NAS")).unwrap();
        let a = |s: &str| s.to_string();
        let j = r.job(&[a("/home/me/Documents"), a(" 192.168.1.50 "), a("/backups/laptop/"), a("me"), a("pw")], r.schedule).unwrap();
        assert_eq!(j.params["dest"], "rsync://me@192.168.1.50/backups/laptop");
        assert_eq!(j.params["password"], "pw");
        assert!(!j.params.contains_key("nas_host"));
    }

    /// The notes example against a real (local, bare) git remote: commit, push, then pull a
    /// change made on "another computer".
    #[test]
    fn git_notes_sync() {
        let d = std::env::temp_dir().join(format!("cadence-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let git = |dir: &std::path::Path, args: &[&str]| {
            let o = std::process::Command::new("git").current_dir(dir).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"]).args(args).output().unwrap();
            assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        };
        git(&d, &["init", "-q", "--bare", "remote.git"]);
        git(&d, &["clone", "-q", "remote.git", "notes"]);
        git(&d, &["clone", "-q", "remote.git", "other"]);
        let notes = d.join("notes");
        std::fs::write(notes.join("idea.md"), "hello").unwrap();

        let ctx = crate::jobs::Ctx { user: "me".into(), uid: 999_999, home: d.clone() };
        let r = RECIPES.iter().find(|r| r.title.contains("git")).unwrap();
        let j = r.job(&[notes.display().to_string()], r.schedule).unwrap();
        let run = || {
            let p = d.join("sync.sh");
            std::fs::write(&p, ctx.script(&j)).unwrap();
            let st = std::process::Command::new("bash").arg(&p).env_remove("DBUS_SESSION_BUS_ADDRESS").status().unwrap();
            (st.code().unwrap(), crate::jobs::last_output(&ctx.log_path(&j)))
        };
        let (rc, out) = run();
        assert_eq!(rc, 0, "{out}");
        git(&d.join("other"), &["pull", "-q", "origin", "HEAD"]);
        assert_eq!(std::fs::read_to_string(d.join("other/idea.md")).unwrap(), "hello");

        // a change from the other computer arrives on the next sync
        std::fs::write(d.join("other/todo.md"), "milk").unwrap();
        git(&d.join("other"), &["add", "-A"]);
        git(&d.join("other"), &["commit", "-q", "-m", "x"]);
        git(&d.join("other"), &["push", "-q", "origin", "HEAD"]);
        let (rc, out) = run();
        assert_eq!(rc, 0, "{out}");
        assert_eq!(std::fs::read_to_string(notes.join("todo.md")).unwrap(), "milk");
    }

    #[test]
    fn mount_names_unescape() { assert_eq!(unescape(r"/media/me/USB\040STICK"), "/media/me/USB STICK"); }

    #[test]
    fn handy_tools_run() {
        let ctx = crate::jobs::Ctx { user: "me".into(), uid: 999_999, home: std::env::temp_dir().join("cadence-tools") };
        let slow_or_needs_setup = ["internet", "disk space", "biggest", "git"];
        for r in RECIPES.iter().filter(|r| (r.category == "Handy tools" || r.title.contains("battery")) && !slow_or_needs_setup.iter().any(|w| r.title.contains(w))) {
            let j = r.job(&[], "manual").unwrap();
            let p = ctx.home.join(format!("{}.sh", j.id));
            std::fs::create_dir_all(&ctx.home).unwrap();
            std::fs::write(&p, ctx.script(&j)).unwrap();
            let st = std::process::Command::new("bash").arg(&p).env_remove("DBUS_SESSION_BUS_ADDRESS").status().unwrap();
            assert!(st.success(), "{}: {}", r.title, crate::jobs::last_output(&ctx.log_path(&j)));
        }
    }
}
