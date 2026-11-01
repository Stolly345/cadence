//! Jobs, the bash scripts they become, and the crontabs that run them.
//! User jobs: script in ~/.local/share/cadence/jobs, line in a marked block of the user crontab.
//! Root jobs: script in /usr/local/lib/cadence (root-owned, so root never runs a user-writable
//! file), line in /etc/cron.d/cadence. Every script wraps its job with logging, a lock against
//! overlapping runs, desktop notifications and an optional dead-man's-switch ping.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{fs, io::Write as _};

pub const ROOT_SCRIPTS: &str = "/usr/local/lib/cadence";
pub const ROOT_LOGS: &str = "/var/log/cadence";
pub const CRON_D: &str = "/etc/cron.d/cadence";
const BEGIN: &str = "# BEGIN cadence: managed by the Cadence app, edits inside this block are overwritten";
const END: &str = "# END cadence";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind { Script, Rsync, Snapshot, Verify, Pull, DeleteOld, DiskAlert, Updates, SortDownloads }

#[derive(Clone, Copy, PartialEq)]
pub enum Input { Text, Multi, Bool, /** one folder per line */ List, /** stored, never put in a script */ Secret }

pub struct Field { pub key: &'static str, pub label: &'static str, pub default: &'static str, pub input: Input }

const fn f(key: &'static str, label: &'static str, default: &'static str, input: Input) -> Field {
    Field { key, label, default, input }
}
const MOUNT: Field = f("require_mount", "Only run if this is mounted (blank = don't check)", "", Input::Text);
const SRC: Field = f("src", "Folders to back up (one per line); each lands in its own folder, by name", "~/Documents", Input::List);
const EXCLUDE: Field = f("exclude", "Skip files and folders named (one pattern per line, e.g. Cache or *.tmp; optional)", "", Input::List);
const PASSWORD: Field = f("password", "NAS password (rsync:// only; blank if none)", "", Input::Secret);

impl Kind {
    pub const ALL: [Kind; 9] = [Kind::Script, Kind::Rsync, Kind::Snapshot, Kind::Verify, Kind::Pull, Kind::DeleteOld,
        Kind::DiskAlert, Kind::Updates, Kind::SortDownloads];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Script => "Bash script",
            Kind::Rsync => "Rsync backup (drive, NAS or SSH)",
            Kind::Snapshot => "Snapshot backup with retention",
            Kind::Verify => "Verify a backup",
            Kind::Pull => "Rsync pull (copy from another computer or NAS)",
            Kind::DeleteOld => "Delete old files (temp, cache, trash)",
            Kind::DiskAlert => "Disk space alert",
            Kind::Updates => "System updates",
            Kind::SortDownloads => "Sort Downloads by type",
        }
    }

    pub fn fields(self) -> &'static [Field] {
        use Input::*;
        match self {
            Kind::Script => const { &[f("body", "Script (plain bash)", "echo hello\n", Multi)] },
            Kind::Rsync => const { &[
                SRC,
                f("dest", "Back up into: a folder, rsync://user@nas/share/path (NAS) or user@host:/path (SSH)", "/media/backup/Cadence Backups", Text),
                PASSWORD,
                f("delete", "Mirror: delete files that are gone from the source", "no", Bool),
                EXCLUDE,
                MOUNT,
            ] },
            Kind::Snapshot => const { &[
                SRC,
                f("dest", "Snapshots folder (on this computer or a mounted drive)", "/media/backup/Cadence Backups/Snapshots", Text),
                EXCLUDE,
                MOUNT,
                f("keep_daily", "Keep daily snapshots", "7", Text),
                f("keep_weekly", "Keep weekly snapshots", "4", Text),
                f("keep_monthly", "Keep monthly snapshots", "12", Text),
            ] },
            Kind::Verify => const { &[
                SRC,
                f("dest", "Where they were backed up into (for snapshots, the snapshots folder's /latest)", "/media/backup/Cadence Backups", Text),
                PASSWORD,
                f("checksum", "Compare file contents (slow), not just size and time", "no", Bool),
                EXCLUDE,
                MOUNT,
            ] },
            Kind::Pull => const { &[
                f("from", "Copy from: user@host:/path (SSH) or rsync://user@nas/share/path (NAS)", "me@192.168.1.50:/var/backups", Text),
                f("dest", "Into this folder", "~/Backups/server", Text),
                PASSWORD,
                f("delete", "Mirror: delete files here that are gone over there", "no", Bool),
                EXCLUDE,
            ] },
            Kind::DeleteOld => const { &[
                f("dir", "In folder", "~/.cache", Text),
                f("days", "Delete files older than (days)", "30", Text),
                f("pattern", "Matching name", "*", Text),
            ] },
            Kind::DiskAlert => const { &[
                f("mounts", "Check (space-separated mount points)", "/", Text),
                f("warn", "Warn at % full", "80", Text),
                f("crit", "Critical at % full", "90", Text),
            ] },
            Kind::Updates => const { &[f("upgrade", "Install updates (off = only notify that updates are available)", "no", Bool)] },
            Kind::SortDownloads => const { &[f("dir", "Folder", "~/Downloads", Text)] },
        }
    }

    /// Updates must be root; sorting your own Downloads must not be (root would own the moved files).
    pub fn forced_root(self) -> Option<bool> {
        match self {
            Kind::Updates => Some(true),
            Kind::SortDownloads => Some(false),
            _ => None,
        }
    }

    pub fn default_schedule(self) -> &'static str {
        match self {
            Kind::DiskAlert => "0 * * * *",
            Kind::Updates => "0 12 * * *",
            Kind::SortDownloads => "*/30 * * * *",
            Kind::Verify => "30 3 * * 0",
            Kind::DeleteOld => "0 4 * * 0",
            _ => "0 2 * * *",
        }
    }

    fn body(self) -> String {
        match self {
            Kind::Script => "set +euo pipefail\n$BODY_SCRIPT".into(),
            Kind::Rsync => with_prep(RSYNC),
            Kind::Snapshot => with_prep(SNAPSHOT),
            Kind::Verify => with_prep(VERIFY),
            Kind::Pull => PULL.into(),
            Kind::DeleteOld => DELETE_OLD.into(),
            Kind::DiskAlert => DISK_ALERT.into(),
            Kind::Updates => UPDATES.into(),
            Kind::SortDownloads => SORT.into(),
        }
    }
}

fn with_prep(body: &str) -> String { body.replace("@PREP@", PREP) }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    pub schedule: String,
    #[serde(default)]
    pub root: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub notify_success: bool,
    #[serde(default)]
    pub ping_url: String,
    /// Run a missed run (computer asleep or off) at the next hourly check.
    #[serde(default = "yes")]
    pub catch_up: bool,
}
fn yes() -> bool { true }

impl Job {
    pub fn new(kind: Kind) -> Job {
        let id = format!("j{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
        Job {
            id,
            name: kind.label().into(),
            kind,
            params: kind.fields().iter().map(|f| (f.key.to_string(), f.default.to_string())).collect(),
            schedule: kind.default_schedule().into(),
            root: kind.forced_root().unwrap_or(false),
            enabled: true,
            notify_success: false,
            ping_url: String::new(),
            catch_up: true,
        }
    }

    pub fn param<'a>(&'a self, f: &'a Field) -> &'a str {
        self.params.get(f.key).map(String::as_str).unwrap_or(f.default)
    }
}

/// Substitute `@KEY@` placeholders in one pass, so substituted text is never itself expanded.
fn fill(t: &str, vals: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = t;
    while let Some(i) = rest.find('@') {
        out += &rest[..i];
        let after = &rest[i + 1..];
        match after.find('@').and_then(|e| vals.iter().find(|(k, _)| *k == &after[..e]).map(|(_, v)| (e, v))) {
            Some((e, v)) => { out += v; rest = &after[e + 1..]; }
            None => { out += "@"; rest = after; }
        }
    }
    out + rest
}

/// Single-quote for bash: nothing inside is expanded.
pub fn sq(s: &str) -> String { format!("'{}'", s.replace('\'', r"'\''")) }

/// Schedules in words, for people who don't read cron. "manual" = only when Run is pressed.
pub const PLAIN: &[(&str, &str)] = &[
    ("Only when I press Run", "manual"),
    ("Every 15 minutes", "*/15 * * * *"),
    ("Every 30 minutes", "*/30 * * * *"),
    ("Every hour", "0 * * * *"),
    ("Every day at noon", "0 12 * * *"),
    ("Every day at 8 PM", "0 20 * * *"),
    ("Every night at 2 AM", "0 2 * * *"),
    ("Every Sunday at noon", "0 12 * * 0"),
    ("First day of every month", "0 12 1 * *"),
    ("When the computer starts", "@reboot"),
];

/// A schedule in words.
pub fn describe(s: &str) -> String {
    let s = s.trim();
    if let Some((w, _)) = PLAIN.iter().find(|(_, c)| *c == s) { return w.to_string(); }
    s.parse::<croner::Cron>().map(|c| c.describe()).unwrap_or_else(|_| s.to_string())
}

/// Shortest gap between runs, in seconds (None for "manual" and "@reboot").
pub fn period(s: &str) -> Option<i64> {
    let c: croner::Cron = s.trim().parse().ok()?;
    let t: Vec<i64> = c.iter_after(chrono::Local::now()).take(8).map(|t| t.timestamp()).collect();
    t.windows(2).map(|w| w[1] - w[0]).min()
}

/// Whether missed runs are made up: only for jobs that run every 2 hours or less often.
pub fn catches_up(j: &Job) -> bool { j.catch_up && scheduled(j) && period(&j.schedule).is_some_and(|p| p >= 7200) }

/// Whether cron runs it (enabled and not run-by-hand only).
pub fn scheduled(j: &Job) -> bool { j.enabled && j.schedule.trim() != "manual" }

/// A cron schedule: a @nickname or exactly five fields. Five, not six: in /etc/cron.d a sixth
/// field would be read as the user to run as.
pub fn check_schedule(s: &str) -> Result<(), String> {
    let s = s.trim();
    if s == "@reboot" || s == "manual" { return Ok(()); }
    if !s.starts_with('@') && s.split_whitespace().count() != 5 {
        return Err("a schedule has 5 fields: minute hour day-of-month month day-of-week".into());
    }
    s.parse::<croner::Cron>().map(|_| ()).map_err(|e| format!("bad schedule: {e}"))
}

/// The next `n` run times, or why the schedule is wrong.
pub fn next_runs(s: &str, n: usize) -> Result<Vec<chrono::DateTime<chrono::Local>>, String> {
    check_schedule(s)?;
    if matches!(s.trim(), "@reboot" | "manual") { return Ok(vec![]); }
    let c: croner::Cron = s.trim().parse().map_err(|e| format!("{e}"))?;
    Ok(c.iter_after(chrono::Local::now()).take(n).collect())
}

/// A one-per-line list, blanks dropped.
pub fn list(s: &str) -> Vec<&str> { s.lines().map(str::trim).filter(|l| !l.is_empty()).collect() }

pub fn validate(j: &Job) -> Result<(), String> {
    if j.name.trim().is_empty() { return Err("give the job a name".into()); }
    check_schedule(&j.schedule)?;
    for f in j.kind.fields() {
        let v = j.param(f).trim();
        let numeric = matches!(f.key, "days" | "warn" | "crit" | "keep_daily" | "keep_weekly" | "keep_monthly");
        if numeric && v.parse::<u32>().is_err() { return Err(format!("“{}” must be a whole number", f.label)); }
        if matches!(f.key, "src" | "dest" | "dir" | "mounts") && v.is_empty() { return Err(format!("“{}” is empty", f.label)); }
    }
    if let Some(src) = j.params.get("src").filter(|_| j.kind.fields().iter().any(|f| f.input == Input::List)) {
        let folders = list(src);
        if folders.is_empty() { return Err("add at least one folder to back up".into()); }
        let mut names = std::collections::HashSet::new();
        for f in &folders {
            let name = f.trim_end_matches('/').rsplit('/').next().unwrap_or("");
            if name.is_empty() { return Err("back up folders inside / rather than / itself".into()); }
            if !names.insert(name.to_string()) {
                return Err(format!("two folders are both called “{name}”: they would land in the same place in the backup"));
            }
        }
    }
    let nas = ["dest", "from"].iter().any(|k| j.params.get(*k).is_some_and(|d| d.trim().starts_with("rsync://")));
    if nas && j.root { return Err("NAS (rsync://) jobs can't run as administrator".into()); }
    if j.kind == Kind::Snapshot && j.param(&j.kind.fields()[1]).contains(':') {
        return Err("snapshots go to a folder on this computer or a mounted drive (use an Rsync job for a NAS or SSH)".into());
    }
    if j.kind == Kind::Pull {
        if !j.param(&j.kind.fields()[0]).contains(':') { return Err("“Copy from” must be another computer: user@host:/path or rsync://…".into()); }
        let d = j.param(&j.kind.fields()[1]).trim();
        if !(d.starts_with('/') || d.starts_with('~')) { return Err("pull into a folder on this computer".into()); }
    }
    if j.ping_url.contains(char::is_whitespace) { return Err("the ping URL has spaces in it".into()); }
    Ok(())
}

#[derive(Clone)]
pub struct Ctx { pub user: String, pub uid: u32, pub home: PathBuf }

impl Ctx {
    pub fn current() -> Ctx {
        use std::os::unix::fs::MetadataExt;
        let uid = fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(1000);
        let user = std::env::var("USER").unwrap_or_else(|_| "nobody".into());
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into());
        Ctx { user, uid, home }
    }

    pub fn config(&self) -> PathBuf { self.home.join(".config/cadence/jobs.json") }
    fn user_scripts(&self) -> PathBuf { self.home.join(".local/share/cadence/jobs") }

    pub fn script_path(&self, j: &Job) -> PathBuf {
        if j.root { Path::new(ROOT_SCRIPTS).join(format!("{}.sh", j.id)) } else { self.user_scripts().join(format!("{}.sh", j.id)) }
    }
    pub fn log_path(&self, j: &Job) -> PathBuf {
        let dir = if j.root { PathBuf::from(ROOT_LOGS) } else { self.home.join(".local/state/cadence") };
        dir.join(format!("{}.log", j.id))
    }

    /// Where a NAS job's password is kept (mode 600), for rsync --password-file.
    pub fn secret_path(&self, j: &Job) -> PathBuf { self.home.join(".config/cadence/secrets").join(&j.id) }

    /// Write (or remove) a job's password file.
    pub fn write_secret(&self, j: &Job) -> Result<(), String> {
        use std::os::unix::fs::OpenOptionsExt;
        let p = self.secret_path(j);
        let pw = j.params.get("password").map(|s| s.trim()).unwrap_or("");
        if pw.is_empty() || j.root { let _ = fs::remove_file(&p); return Ok(()); }
        fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&p);
        let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&p).map_err(|e| e.to_string())?;
        f.write_all(pw.as_bytes()).map_err(|e| e.to_string())
    }

    fn expand(&self, s: &str) -> String {
        let h = self.home.display().to_string();
        if s == "~" { h } else if let Some(r) = s.strip_prefix("~/") { format!("{h}/{r}") } else { s.to_string() }
    }

    pub fn script(&self, j: &Job) -> String {
        let mut vars = String::new();
        for f in j.kind.fields() {
            let v = j.param(f);
            match f.input {
                Input::Multi => {} // the script body, inserted below
                Input::Secret => {
                    let file = if v.trim().is_empty() { String::new() } else { self.secret_path(j).display().to_string() };
                    vars += &format!("PASSFILE={}\n", sq(&file));
                }
                Input::List => {
                    let items: Vec<String> = list(v).iter().map(|x| sq(&self.expand(x))).collect();
                    vars += &format!("{}=({})\n", f.key.to_uppercase(), items.join(" "));
                }
                _ => vars += &format!("{}={}\n", f.key.to_uppercase(), sq(&self.expand(v.trim()))),
            }
        }
        // a Script example's own answers (e.g. `notes` → $NOTES)
        if j.kind == Kind::Script {
            for (k, v) in j.params.iter().filter(|(k, _)| *k != "body") {
                if k.chars().all(|c| c.is_ascii_lowercase() || c == '_') && !k.is_empty() {
                    vars += &format!("{}={}\n", k.to_uppercase(), sq(&self.expand(v.trim())));
                }
            }
        }
        let body = j.kind.body().replace("$BODY_SCRIPT", j.params.get("body").map(String::as_str).unwrap_or(""));
        let period = period(&j.schedule).unwrap_or(0).to_string();
        let name = j.name.replace('\n', " ");
        let (log, ping, user, uid, home) = (
            sq(&self.log_path(j).display().to_string()), sq(j.ping_url.trim()), sq(&self.user),
            self.uid.to_string(), sq(&self.home.display().to_string()),
        );
        fill(WRAPPER, &[
            ("NAME", &name), ("QNAME", &sq(&j.name)), ("LOG", &log), ("PING", &ping), ("USER", &user),
            ("UID", &uid), ("HOME", &home), ("VARS", &vars), ("BODY", &body),
            ("NOTIFY_SUCCESS", if j.notify_success { "1" } else { "0" }), ("PERIOD", &period),
        ])
    }

    /// The job's cron line, plus an hourly catch-up check when it makes up missed runs.
    fn cron_lines(&self, j: &Job) -> Vec<String> {
        let who = if j.root { "root " } else { "" };
        let script = sq(&self.script_path(j).display().to_string());
        let mut v = vec![format!("{} {who}/bin/bash {script} >/dev/null 2>&1", j.schedule.trim())];
        if catches_up(j) { v.push(format!("7 * * * * {who}/bin/bash {script} --catch-up >/dev/null 2>&1")); }
        v
    }

    /// Write every script and both crontabs. Asks for the admin password only when something
    /// root-owned actually changes. Returns what was done.
    pub fn apply(&self, jobs: &[Job]) -> Result<String, String> {
        for j in jobs { validate(j).map_err(|e| format!("{}: {e}", j.name))?; }
        let (root, user): (Vec<&Job>, Vec<&Job>) = jobs.iter().partition(|j| j.root);

        // user scripts: rewrite the folder so deleted jobs' scripts go too
        let dir = self.user_scripts();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for e in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            if e.path().extension().is_some_and(|x| x == "sh") { let _ = fs::remove_file(e.path()); }
        }
        for j in &user { write_exec(&self.script_path(j), &self.script(j))?; }
        let _ = fs::remove_dir_all(self.home.join(".config/cadence/secrets"));
        for j in &user { self.write_secret(j)?; }

        let block: Vec<String> = user.iter().filter(|j| scheduled(j)).flat_map(|j| self.cron_lines(j)).collect();
        let current = read_crontab()?;
        let merged = merge_block(&current, &block);
        if merged != current { write_crontab(&merged)?; }
        let mut done = format!("{} user job(s) scheduled", user.iter().filter(|j| scheduled(j)).count());

        // root: stage, compare with what's installed, install in one pkexec call if different
        let cron_d = format!(
            "# Managed by the Cadence app ({}). Edit jobs there, not here.\n{}",
            self.user,
            root.iter().filter(|j| scheduled(j)).flat_map(|j| self.cron_lines(j)).map(|l| l + "\n").collect::<String>()
        );
        let scripts: Vec<(String, String)> = root.iter().map(|j| (format!("{}.sh", j.id), self.script(j))).collect();
        let installed_same = fs::read_to_string(CRON_D).ok().as_deref() == Some(&cron_d)
            && scripts.iter().all(|(n, s)| fs::read_to_string(Path::new(ROOT_SCRIPTS).join(n)).ok().as_deref() == Some(s))
            && fs::read_dir(ROOT_SCRIPTS).map(|d| d.count()).unwrap_or(0) == scripts.len();
        let nothing_root = root.is_empty() && !Path::new(CRON_D).exists();
        if !installed_same && !nothing_root {
            let stage = self.home.join(".cache/cadence/stage");
            let _ = fs::remove_dir_all(&stage);
            fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
            for (n, s) in &scripts { write_exec(&stage.join(n), s)?; }
            fs::write(stage.join("cron"), &cron_d).map_err(|e| e.to_string())?;
            let st = Command::new("pkexec")
                .args(["/bin/sh", "-c", INSTALL_ROOT, "sh"])
                .arg(&stage)
                .status()
                .map_err(|e| format!("pkexec: {e}"))?;
            let _ = fs::remove_dir_all(&stage);
            if !st.success() { return Err(format!("{done}; root jobs NOT installed (authorization refused or failed)")); }
        }
        let n = root.iter().filter(|j| scheduled(j)).count();
        if n > 0 || !nothing_root { done += &format!(", {n} root job(s) scheduled"); }
        Ok(done)
    }

    /// The command that runs a job's script by hand (via pkexec for root jobs).
    pub fn run_argv(j: &Job, script: &Path) -> Vec<std::ffi::OsString> {
        let mut v: Vec<std::ffi::OsString> = if j.root { vec!["pkexec".into()] } else { vec![] };
        v.push("/bin/bash".into());
        v.push(script.into());
        v
    }
}

/// Last run of a job: (unix time, exit code).
pub fn last_run(log: &Path) -> Option<(i64, i32)> {
    let s = fs::read_to_string(log.with_extension("log.status")).ok()?;
    let mut it = s.split_whitespace();
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// The output of the most recent run, from its log.
pub fn last_output(log: &Path) -> String {
    let s = fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = s.lines().collect();
    let from = lines.iter().rposition(|l| l.starts_with("=== ") && l.ends_with(" start")).map_or(0, |i| i + 1);
    lines[from..].iter().filter(|l| !l.starts_with("=== ")).copied().collect::<Vec<_>>().join("\n")
}

pub fn load(path: &Path) -> Vec<Job> {
    fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(path: &Path, jobs: &[Job]) -> Result<(), String> {
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("json.tmp");
    let _ = fs::remove_file(&tmp);
    // 600: NAS passwords are in here
    let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(&serde_json::to_vec_pretty(jobs).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn write_exec(p: &Path, s: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::write(p, s).map_err(|e| format!("{}: {e}", p.display()))?;
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

fn read_crontab() -> Result<String, String> {
    let o = Command::new("crontab").arg("-l").output()
        .map_err(|_| "cron isn't installed, so jobs can't be scheduled: install the cron package (cronie on Fedora/Arch)".to_string())?;
    if o.status.success() { return Ok(String::from_utf8_lossy(&o.stdout).into()); }
    let err = String::from_utf8_lossy(&o.stderr);
    if err.contains("no crontab") { Ok(String::new()) } else { Err(format!("crontab -l: {err}")) }
}

fn write_crontab(s: &str) -> Result<(), String> {
    let mut c = Command::new("crontab").arg("-").stdin(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    c.stdin.take().unwrap().write_all(s.as_bytes()).map_err(|e| e.to_string())?;
    if c.wait().map_err(|e| e.to_string())?.success() { Ok(()) } else { Err("crontab rejected the new table".into()) }
}

/// Replace our marked block in a crontab, leaving every other line alone.
pub fn merge_block(existing: &str, lines: &[String]) -> String {
    let mut out = String::new();
    let mut inside = false;
    for l in existing.lines() {
        if l == BEGIN { inside = true; continue; }
        if inside { if l == END { inside = false; } continue; }
        out += l;
        out += "\n";
    }
    if !lines.is_empty() {
        out += &format!("{BEGIN}\n{}\n{END}\n", lines.join("\n"));
    }
    out
}

const INSTALL_ROOT: &str = r#"set -e
install -d -m 755 /usr/local/lib/cadence /var/log/cadence
rm -f /usr/local/lib/cadence/*.sh
for f in "$1"/*.sh; do [ -e "$f" ] && install -m 755 -o root -g root "$f" /usr/local/lib/cadence/; done
install -m 644 -o root -g root "$1/cron" /etc/cron.d/cadence
"#;

const WRAPPER: &str = r#"#!/bin/bash
# Generated by Cadence for the job "@NAME@". Edit the job in Cadence: this file is overwritten.
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
JOB_NAME=@QNAME@
LOG=@LOG@
USER_HOME=@HOME@
NOTIFY_SUCCESS=@NOTIFY_SUCCESS@
PING_URL=@PING@
PERIOD=@PERIOD@

notify() { # urgency message
  local bus=unix:path=/run/user/@UID@/bus
  if [ "$(id -u)" = 0 ]; then
    runuser -u @USER@ -- env DBUS_SESSION_BUS_ADDRESS=$bus notify-send -a Cadence -u "$1" "$JOB_NAME" "$2" || true
  else
    DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-$bus} notify-send -a Cadence -u "$1" "$JOB_NAME" "$2" || true
  fi
}
fail() { echo "$*" >&2; exit 1; }
# not a failure: the drive or NAS isn't there right now (unplugged, away from home). Exit 75.
skip() { echo "Skipped: $*"; exit 75; }
ping_hc() { if [ -n "$PING_URL" ]; then curl -fsS -m 10 --retry 3 "$PING_URL$1" >/dev/null 2>&1 || true; fi; }
mount_guard() {
  if [ -n "${REQUIRE_MOUNT:-}" ] && ! mountpoint -q "$REQUIRE_MOUNT"; then
    skip "$REQUIRE_MOUNT isn't connected"
  fi
}
transport() { # rsync options for reaching $1 (a NAS, an SSH host or a local folder)
  case "$1" in
    rsync://*) opts+=(--no-owner --no-group --contimeout=20)
               if [ -n "${PASSFILE:-}" ]; then opts+=(--password-file="$PASSFILE"); fi ;;
    /*) ;;
    *:*) opts+=(-e "ssh -o BatchMode=yes -o ConnectTimeout=20") ;;
  esac
  local x
  for x in ${EXCLUDE[@]+"${EXCLUDE[@]}"}; do opts+=(--exclude="$x"); done
}
reach_guard() { # skip when a NAS / SSH destination can't be reached
  local d=$1 host port
  case "$d" in
    rsync://*) host=${d#rsync://}; host=${host%%/*}; host=${host##*@}; port=873
               case "$host" in *:*) port=${host##*:}; host=${host%:*} ;; esac ;;
    /*) return 0 ;;
    *:*) host=${d%%:*}; host=${host##*@}; port=22 ;;
    *) return 0 ;;
  esac
  getent hosts "$host" >/dev/null || { [[ "$host" =~ ^[0-9.]+$ ]] || return 0; } # ssh-config aliases: let ssh decide
  timeout 5 bash -c 'exec 3<>"/dev/tcp/$1/$2"' _ "$host" "$port" 2>/dev/null || skip "$host isn't reachable (not on the same network?)"
}

mkdir -p "$(dirname "$LOG")"
if [ -f "$LOG" ] && [ "$(stat -c %s "$LOG")" -gt 1048576 ]; then tail -n 2000 "$LOG" >"$LOG.tmp" && mv "$LOG.tmp" "$LOG"; fi
# --catch-up (hourly): run only if the last success and the last failure are both older than
# the job's period, i.e. a run was missed while the computer was asleep or off
CATCH_UP=0
if [ "${1:-}" = --catch-up ]; then
  CATCH_UP=1
  age() { if [ -f "$1" ]; then echo $(( $(date +%s) - $(stat -c %Y "$1") )); else echo 999999999; fi; }
  if [ "$(age "$LOG.ok")" -lt "$PERIOD" ] || [ "$(age "$LOG.failed")" -lt "$PERIOD" ]; then exit 0; fi
fi
exec 9>"$LOG.lock"
if ! flock -n 9; then
  if [ $CATCH_UP = 0 ]; then echo "=== $(date -Is) skipped: the previous run is still going" >>"$LOG"; fi
  exit 0
fi

echo "=== $(date -Is) start" >>"$LOG"
if [ $CATCH_UP = 1 ]; then echo "(catching up: the last scheduled run was missed)" >>"$LOG"; fi
(
set --
set -euo pipefail
@VARS@
@BODY@
) >>"$LOG" 2>&1 9>&-
rc=$?
echo "=== $(date -Is) exit $rc" >>"$LOG"
echo "$(date +%s) $rc" >"$LOG.status"
if [ $rc -eq 75 ]; then
  # skipped: quiet, unless there's been no successful run for a week (then once a day)
  if [ -z "$(find "$LOG.ok" -mtime -7 2>/dev/null)" ] && [ -z "$(find "$LOG.nagged" -mtime -1 2>/dev/null)" ]; then
    notify critical "No successful run for over a week. $(grep '^Skipped: ' "$LOG" | tail -n 1)"
    touch "$LOG.nagged"
  fi
elif [ $rc -ne 0 ]; then
  touch "$LOG.failed"
  notify critical "Failed (exit $rc): $(grep -v '^===' "$LOG" | tail -n 2)"
  ping_hc /fail
else
  touch "$LOG.ok"
  if [ "$NOTIFY_SUCCESS" = 1 ]; then notify normal "Finished OK"; fi
  ping_hc ""
fi
exit $rc
"#;

// Shared by the backup kinds: check each folder, then rsync options for the destination.
// rsync exit 24 = files vanished mid-run (normal for a live home folder): not a failure.
const PREP: &str = r#"mount_guard
reach_guard "$DEST"
folders=()
for s in "${SRC[@]}"; do [ -d "$s" ] || fail "$s doesn't exist"; folders+=("${s%/}"); done
opts=(-a --human-readable)
transport "$DEST"
"#;

// The other way round: another computer's folder into one here.
const PULL: &str = r#"reach_guard "$FROM"
opts=(-a --human-readable --stats)
transport "$FROM"
if [ "$DELETE" = yes ]; then opts+=(--delete); fi
mkdir -p "$DEST"
rsync "${opts[@]}" "${FROM%/}/" "${DEST%/}/" || { rc=$?; [ $rc -eq 24 ] || exit $rc; }"#;

// Each folder lands in DEST/<its name>.
const RSYNC: &str = r#"@PREP@
if [ "$DELETE" = yes ]; then opts+=(--delete); fi
case "$DEST" in /*) mkdir -p "$DEST" ;; esac
rsync "${opts[@]}" --stats "${folders[@]}" "${DEST%/}/" || { rc=$?; [ $rc -eq 24 ] || exit $rc; }"#;

// Each snapshot is a full tree; unchanged files are hard links into the previous one.
// Retention keeps the newest snapshot of each of the last N days / ISO weeks / months.
const SNAPSHOT: &str = r#"@PREP@
mkdir -p "$DEST"
find "$DEST" -maxdepth 1 -name '*.partial' -exec rm -rf {} +
ts=$(date +%F_%H%M%S)
link=()
if [ -d "$DEST/latest" ]; then link=(--link-dest="$DEST/latest"); fi
rsync "${opts[@]}" --delete --stats "${link[@]}" "${folders[@]}" "$DEST/$ts.partial/" || { rc=$?; [ $rc -eq 24 ] || exit $rc; }
mv "$DEST/$ts.partial" "$DEST/$ts"
ln -sfn "$ts" "$DEST/latest"
echo "snapshot: $DEST/$ts"

if [ "$KEEP_DAILY" -lt 1 ]; then KEEP_DAILY=1; fi
declare -A keep=() seen=()
nd=0; nw=0; nm=0
snaps=$(ls -1 "$DEST" | grep -E '^[0-9]{4}-[0-9]{2}-[0-9]{2}_[0-9]{6}$' | sort -r || true)
for s in $snaps; do
  d=${s%_*}; w=$(date -d "$d" +%G-W%V); m=${d%-*}
  if [ $nd -lt "$KEEP_DAILY" ] && [ -z "${seen[d$d]:-}" ]; then seen[d$d]=1; nd=$((nd+1)); keep[$s]=1; fi
  if [ $nw -lt "$KEEP_WEEKLY" ] && [ -z "${seen[w$w]:-}" ]; then seen[w$w]=1; nw=$((nw+1)); keep[$s]=1; fi
  if [ $nm -lt "$KEEP_MONTHLY" ] && [ -z "${seen[m$m]:-}" ]; then seen[m$m]=1; nm=$((nm+1)); keep[$s]=1; fi
done
for s in $snaps; do
  if [ -z "${keep[$s]:-}" ]; then echo "pruning: $s"; rm -rf "${DEST:?}/$s"; fi
done"#;

// Reports what each folder has that its backup lacks or has different. Extra files in the
// backup are not a difference (a non-mirror backup keeps them on purpose).
const VERIFY: &str = r#"@PREP@
opts+=(-n --itemize-changes)
if [ "$CHECKSUM" = yes ]; then opts+=(-c); fi
total=0
for s in "${folders[@]}"; do
  diff=$(rsync "${opts[@]}" "$s/" "${DEST%/}/${s##*/}/" | grep -v '^\.d' || true)
  if [ -n "$diff" ]; then
    echo "$diff" | head -n 50 | sed "s|^|${s##*/}: |"
    total=$((total + $(echo "$diff" | wc -l)))
  else
    echo "✓ ${s##*/} matches its backup"
  fi
done
if [ "$total" -gt 0 ]; then fail "$total file(s) differ between your folders and the backup"; fi"#;

// Age is the later of modified and changed time, so things moved into the trash today are kept.
const DELETE_OLD: &str = r#"DIR=${DIR%/}
case "$DIR" in ""|/|"$USER_HOME"|/home|/root|/etc|/usr|/var) fail "refusing to clean $DIR" ;; esac
[ -d "$DIR" ] || fail "$DIR does not exist"
n=$(find "$DIR" -xdev -mindepth 1 -type f -name "$PATTERN" -mtime +"$DAYS" -ctime +"$DAYS" -print -delete | tee /dev/stderr | wc -l)
find "$DIR" -xdev -mindepth 1 -type d -empty -mtime +"$DAYS" -delete || true
echo "deleted $n file(s)""#;

const DISK_ALERT: &str = r#"for m in $MOUNTS; do
  pct=$(df -P "$m" | awk 'NR==2 {gsub("%","",$5); print $5}')
  echo "$m: ${pct}% full"
  if [ "$pct" -ge "$CRIT" ]; then notify critical "$m is ${pct}% full"
  elif [ "$pct" -ge "$WARN" ]; then notify normal "$m is ${pct}% full"; fi
done"#;

const UPDATES: &str = r#"if command -v apt-get >/dev/null; then
  export DEBIAN_FRONTEND=noninteractive
  apt=(apt-get -o DPkg::Lock::Timeout=600)
  "${apt[@]}" update -qq
  n=$("${apt[@]}" -s upgrade | grep -c '^Inst' || true)
  install() { "${apt[@]}" -y -o Dpkg::Options::=--force-confold upgrade; }
elif command -v dnf >/dev/null; then
  n=$(dnf -q check-update 2>/dev/null | grep -cE '^[[:alnum:]]' || true)
  install() { dnf -y upgrade; }
elif command -v zypper >/dev/null; then
  zypper -n -q refresh
  n=$(zypper -n -q list-updates 2>/dev/null | grep -c '^v ' || true)
  install() { zypper -n update; }
else
  fail "Updates work with apt (Ubuntu/Debian), dnf (Fedora) and zypper (openSUSE); this system has none of them"
fi
echo "$n package(s) can be updated"
if [ "$UPGRADE" = yes ] && [ "$n" -gt 0 ]; then
  install
  notify normal "Installed $n update(s)"
elif [ "$n" -gt 0 ]; then
  notify normal "$n update(s) available"
fi
if [ -f /var/run/reboot-required ]; then notify normal "Restart the computer to finish updating"; fi"#;

// Uses Magpie when installed (undoable runs). Otherwise moves top-level files into
// Images/Documents/... Never touches files changed in the last hour (may still be downloading),
// unknown types, folders or hidden files, and never overwrites: a clash becomes "name (1).ext".
const SORT: &str = r#"[ -d "$DIR" ] || fail "$DIR does not exist"
if command -v magpie-cli >/dev/null; then magpie-cli plan "$DIR" --style by-type --yes; exit 0; fi
shopt -s nullglob nocasematch
moved=0
for f in "$DIR"/*; do
  [ -f "$f" ] && [ ! -L "$f" ] || continue
  if [ -n "$(find "$f" -mmin -60)" ]; then continue; fi
  name=${f##*/}
  case "$name" in
    *.part|*.crdownload|*.download|*.partial|*.tmp) continue ;;
    *.jpg|*.jpeg|*.png|*.gif|*.webp|*.heic|*.svg|*.bmp|*.tif|*.tiff) sub=Images ;;
    *.pdf|*.doc|*.docx|*.odt|*.txt|*.rtf|*.md|*.xls|*.xlsx|*.ods|*.csv|*.ppt|*.pptx|*.odp|*.epub) sub=Documents ;;
    *.mp3|*.flac|*.wav|*.ogg|*.m4a|*.aac|*.opus) sub=Music ;;
    *.mp4|*.mkv|*.mov|*.avi|*.webm|*.m4v) sub=Videos ;;
    *.zip|*.tar|*.gz|*.tgz|*.xz|*.bz2|*.7z|*.rar|*.zst) sub=Archives ;;
    *.deb|*.rpm|*.appimage|*.flatpakref|*.run|*.exe|*.msi|*.iso|*.img) sub=Installers ;;
    *) continue ;;
  esac
  if [[ "$name" == ?*.* ]]; then base=${name%.*}; ext=".${name##*.}"; else base=$name; ext=""; fi
  mkdir -p "$DIR/$sub"
  target="$DIR/$sub/$name"; n=1
  while [ -e "$target" ]; do target="$DIR/$sub/$base ($n)$ext"; n=$((n+1)); done
  mv -n -- "$f" "$target"
  echo "$name → $sub/${target##*/}"
  moved=$((moved+1))
done
echo "Moved $moved file(s)""#;

#[cfg(test)]
mod tests {
    use super::*;
    fn ctx() -> (Ctx, PathBuf) {
        let d = std::env::temp_dir().join(format!("cadence-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        (Ctx { user: "nobody".into(), uid: 999_999, home: d.clone() }, d)
    }

    /// Generate the job's script and run it the way cron would. Returns (exit code, log).
    fn run(c: &Ctx, j: &Job) -> (i32, String) { run_with(c, j, &[]) }

    fn run_with(c: &Ctx, j: &Job, args: &[&str]) -> (i32, String) {
        let p = c.home.join(format!("{}.sh", j.id));
        fs::write(&p, c.script(j)).unwrap();
        let st = Command::new("/bin/bash").arg(&p).args(args).env("PATH", "/usr/bin:/bin").env_remove("DBUS_SESSION_BUS_ADDRESS").status().unwrap();
        (st.code().unwrap(), fs::read_to_string(c.log_path(j)).unwrap_or_default())
    }

    fn job(kind: Kind, params: &[(&str, &str)]) -> Job {
        let mut j = Job::new(kind);
        for (k, v) in params { j.params.insert(k.to_string(), v.to_string()); }
        j
    }

    #[test]
    fn crontab_block_replaces_only_ours() {
        let mine = "MAILTO=me\n0 1 * * * /usr/bin/true\n";
        let a = merge_block(mine, &["0 2 * * * x".into()]);
        assert!(a.starts_with(mine) && a.contains("0 2 * * * x"));
        let b = merge_block(&a, &["5 5 * * * y".into()]);
        assert!(b.contains("y") && !b.contains(" x\n") && b.starts_with(mine));
        assert_eq!(merge_block(&b, &[]), mine);
    }

    #[test]
    fn schedules() {
        assert!(check_schedule("0 2 * * *").is_ok());
        assert!(check_schedule("@reboot").is_ok());
        assert!(check_schedule("@daily").is_ok());
        assert!(check_schedule("0 0 2 * * *").is_err(), "six fields would be a user in cron.d");
        assert!(check_schedule("61 * * * *").is_err());
        assert!(check_schedule("0 2 * * *\n* * * * * evil").is_err());
        assert_eq!(next_runs("*/15 * * * *", 3).unwrap().len(), 3);
    }

    #[test]
    fn quoting_survives_hostile_names() {
        let (c, d) = ctx();
        let mut j = job(Kind::Script, &[("body", "echo ran")]);
        j.name = "it's $(touch pwned) `x` @BODY@".into();
        let (rc, log) = run(&c, &j);
        assert_eq!(rc, 0);
        assert!(log.contains("ran") && !d.join("pwned").exists() && !Path::new("pwned").exists());
    }

    #[test]
    fn failure_is_logged_with_exit_code() {
        let (c, _) = ctx();
        let j = job(Kind::Script, &[("body", "echo boom >&2; exit 3")]);
        let (rc, log) = run(&c, &j);
        assert_eq!(rc, 3);
        assert!(log.contains("boom") && log.contains("exit 3"));
        assert_eq!(last_run(&c.log_path(&j)).unwrap().1, 3);
        let j2 = Job { id: j.id.clone(), ..job(Kind::Script, &[("body", "echo second")]) };
        run(&c, &j2);
        assert_eq!(last_output(&c.log_path(&j)), "second");
    }

    #[test]
    fn rsync_backs_up_several_folders_and_skips_when_drive_missing() {
        let (c, d) = ctx();
        for (dir, file) in [("Documents/sub", "a.txt"), ("Pictures", "p.jpg"), ("Pictures/Cache", "junk")] {
            fs::create_dir_all(d.join(dir)).unwrap();
            fs::write(d.join(dir).join(file), "a").unwrap();
        }
        let src = format!("{}\n\n{}/\n", d.join("Documents").display(), d.join("Pictures").display());
        let dest = d.join("dest").display().to_string();

        // drive not connected: skipped (75), not failed, and nothing written
        let guarded = job(Kind::Rsync, &[("src", &src), ("dest", &dest), ("require_mount", &dest)]);
        let (rc, log) = run(&c, &guarded);
        assert_eq!(rc, 75, "{log}");
        assert!(log.contains("Skipped:") && !d.join("dest").exists());
        assert!(!c.log_path(&guarded).with_extension("log.ok").exists());

        let j = job(Kind::Rsync, &[("src", &src), ("dest", &dest), ("exclude", "Cache\n*.tmp")]);
        assert_eq!(validate(&j), Ok(()));
        assert_eq!(run(&c, &j).0, 0);
        assert_eq!(fs::read_to_string(d.join("dest/Documents/sub/a.txt")).unwrap(), "a");
        assert_eq!(fs::read_to_string(d.join("dest/Pictures/p.jpg")).unwrap(), "a");
        assert!(!d.join("dest/Pictures/Cache").exists(), "excluded");
        assert!(c.log_path(&j).with_extension("log.ok").exists());

        // verify: matches, then catches a changed file in the second folder
        let v = job(Kind::Verify, &[("src", &src), ("dest", &dest), ("checksum", "yes"), ("exclude", "Cache")]);
        assert_eq!(run(&c, &v).0, 0);
        fs::write(d.join("Pictures/p.jpg"), "b").unwrap();
        let (rc, log) = run(&c, &v);
        assert_eq!(rc, 1);
        assert!(log.contains("Pictures: ") && log.contains("✓ Documents"), "{log}");

        // a missing folder is a failure, not a silent skip
        let gone = job(Kind::Rsync, &[("src", &d.join("nope").display().to_string()), ("dest", &dest)]);
        assert_eq!(run(&c, &gone).0, 1);
    }

    #[test]
    fn missed_runs_catch_up_once() {
        let (c, _) = ctx();
        let mut j = job(Kind::Script, &[("body", "echo ran")]);
        j.schedule = "0 2 * * *".into();
        assert!(catches_up(&j));
        let lines = c.cron_lines(&j);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].starts_with("7 * * * * ") && lines[1].contains("--catch-up"));

        // never ran: the hourly check runs it
        let (rc, log) = run_with(&c, &j, &["--catch-up"]);
        assert_eq!((rc, log.matches("ran").count()), (0, 1), "{log}");
        assert!(log.contains("catching up"));
        // it just succeeded: the next hourly checks do nothing
        let (_, log) = run_with(&c, &j, &["--catch-up"]);
        assert_eq!(log.matches("\nran").count(), 1, "{log}");
        // a scheduled (not catch-up) run always runs
        let (_, log) = run(&c, &j);
        assert_eq!(log.matches("\nran").count(), 2, "{log}");

        // a failing job is retried by catch-up once per period, not every hour
        let mut bad = job(Kind::Script, &[("body", "echo boom; exit 2")]);
        bad.schedule = "0 2 * * *".into();
        assert_eq!(run_with(&c, &bad, &["--catch-up"]).0, 2);
        let (rc, log) = run_with(&c, &bad, &["--catch-up"]);
        assert_eq!((rc, log.matches("boom").count()), (0, 1), "{log}");

        // no catch-up for frequent, manual or switched-off jobs
        for (sched, on) in [("*/15 * * * *", true), ("manual", true), ("@reboot", true), ("0 2 * * *", false)] {
            let mut k = j.clone();
            k.schedule = sched.into();
            k.catch_up = on;
            assert!(!catches_up(&k), "{sched}");
            assert!(c.cron_lines(&k).len() == 1);
        }
        // the script never sees --catch-up as its own argument
        let mut args = job(Kind::Script, &[("body", "echo \"args=$#\"")]);
        args.schedule = "0 2 * * *".into();
        assert!(run_with(&c, &args, &["--catch-up"]).1.contains("args=0"));
    }

    #[test]
    fn same_named_folders_are_refused() {
        let j = job(Kind::Rsync, &[("src", "/a/Docs\n/b/Docs"), ("dest", "/x")]);
        assert!(validate(&j).unwrap_err().contains("Docs"));
        let j = job(Kind::Rsync, &[("src", "/"), ("dest", "/x")]);
        assert!(validate(&j).is_err());
        let mut nas = job(Kind::Rsync, &[("src", "/a"), ("dest", "rsync://nas/backup")]);
        nas.root = true;
        assert!(validate(&nas).is_err(), "root can't use the user's password file");
    }

    /// A real rsync daemon on a spare port stands in for the NAS: password auth, then an
    /// unreachable NAS is a skip.
    #[test]
    fn nas_backup_over_rsync_daemon() {
        let (c, d) = ctx();
        fs::create_dir_all(d.join("Documents")).unwrap();
        fs::write(d.join("Documents/a.txt"), "nas").unwrap();
        fs::create_dir_all(d.join("nas")).unwrap();
        let secrets = d.join("rsyncd.secrets");
        fs::write(&secrets, "me:hunter2\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&secrets, fs::Permissions::from_mode(0o600)).unwrap();
        // a port the OS says is free right now
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        fs::write(d.join("rsyncd.conf"), format!(
            "use chroot = no\npid file = {0}/rsyncd.pid\n[backups]\npath = {0}/nas\nread only = no\nauth users = me\nsecrets file = {1}\n",
            d.display(), secrets.display())).unwrap();
        let mut daemon = Command::new("rsync").args(["--daemon", "--no-detach", "--address=127.0.0.1"])
            .arg(format!("--port={port}")).arg(format!("--config={}", d.join("rsyncd.conf").display()))
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(fs::File::create(d.join("rsyncd.err")).unwrap()).spawn().unwrap();
        for _ in 0..50 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { break; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        let src = d.join("Documents").display().to_string();
        let url = format!("rsync://me@127.0.0.1:{port}/backups/laptop");
        let wrong = job(Kind::Rsync, &[("src", &src), ("dest", &url), ("password", "nope")]);
        c.write_secret(&wrong).unwrap();
        let (rc, log) = run(&c, &wrong);
        assert_ne!(rc, 0, "{log}");
        assert_ne!(rc, 75, "a wrong password is a failure, not a skip: {log}\ndaemon: {}", fs::read_to_string(d.join("rsyncd.err")).unwrap_or_default());

        let j = job(Kind::Rsync, &[("src", &src), ("dest", &url), ("password", "hunter2")]);
        c.write_secret(&j).unwrap();
        assert_eq!(fs::metadata(c.secret_path(&j)).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(!c.script(&j).contains("hunter2"), "the password must not be in the script");
        let (rc, log) = run(&c, &j);
        assert_eq!(rc, 0, "{log}");
        assert_eq!(fs::read_to_string(d.join("nas/laptop/Documents/a.txt")).unwrap(), "nas");

        let v = job(Kind::Verify, &[("src", &src), ("dest", &url), ("password", "hunter2")]);
        c.write_secret(&v).unwrap();
        assert_eq!(run(&c, &v).0, 0);

        // pull it back the other way, into a folder here
        let pull = job(Kind::Pull, &[("from", &url), ("dest", &d.join("pulled").display().to_string()), ("password", "hunter2")]);
        assert_eq!(validate(&pull), Ok(()));
        c.write_secret(&pull).unwrap();
        let (rc, log) = run(&c, &pull);
        assert_eq!(rc, 0, "{log}");
        assert_eq!(fs::read_to_string(d.join("pulled/Documents/a.txt")).unwrap(), "nas");

        daemon.kill().unwrap();
        let _ = daemon.wait();
        let (rc, log) = run(&c, &j);
        assert_eq!(rc, 75, "NAS away = skipped: {log}");
    }

    #[test]
    fn snapshots_hard_link_and_prune() {
        let (c, d) = ctx();
        fs::create_dir_all(d.join("src")).unwrap();
        fs::write(d.join("src/f"), "x").unwrap();
        let snaps = d.join("snaps");
        fs::create_dir_all(&snaps).unwrap();
        // fake history: 10 consecutive days in March, plus one in January
        for day in 1..=10 { fs::create_dir(snaps.join(format!("2026-03-{day:02}_020000"))).unwrap(); }
        fs::create_dir(snaps.join("2026-01-15_020000")).unwrap();
        fs::create_dir(snaps.join("not-a-snapshot")).unwrap();
        let p = |k: &str, v: &str| (k.to_string(), v.to_string());
        let mut j = job(Kind::Snapshot, &[]);
        j.params.extend([p("src", &d.join("src").display().to_string()), p("dest", &snaps.display().to_string()),
            p("keep_daily", "3"), p("keep_weekly", "0"), p("keep_monthly", "2")]);
        assert_eq!(run(&c, &j).0, 0);
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert_eq!(run(&c, &j).0, 0);

        let mut names: Vec<String> = fs::read_dir(&snaps).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        names.sort();
        // daily: today's newest + 03-10 + 03-09; monthly: this month (today) + March (03-10)
        let today: Vec<_> = names.iter().filter(|n| n.starts_with(&chrono::Local::now().format("%F").to_string())).collect();
        assert_eq!(today.len(), 1, "{names:?}");
        assert!(names.contains(&"2026-03-10_020000".into()) && names.contains(&"2026-03-09_020000".into()), "{names:?}");
        assert!(!names.contains(&"2026-03-08_020000".into()) && !names.contains(&"2026-01-15_020000".into()), "{names:?}");
        assert!(names.contains(&"not-a-snapshot".into()) && names.contains(&"latest".into()));
        assert_eq!(fs::read_to_string(snaps.join("latest/src/f")).unwrap(), "x");
    }

    #[test]
    fn delete_old_keeps_new_and_refuses_home() {
        let (c, d) = ctx();
        let dir = d.join("cache");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("new"), "").unwrap();
        let j = job(Kind::DeleteOld, &[("dir", &dir.display().to_string()), ("days", "1")]);
        assert_eq!(run(&c, &j).0, 0);
        assert!(dir.join("new").exists());
        let home = job(Kind::DeleteOld, &[("dir", "~"), ("days", "0")]);
        assert_eq!(run(&c, &home).0, 1);
    }

    #[test]
    fn sort_downloads_builtin() {
        let (c, d) = ctx();
        let dl = d.join("dl");
        fs::create_dir_all(dl.join("Images")).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        for n in ["a.PDF", "pic.png", "Images/pic.png", "new.pdf", "big.iso.part", "noext", "x.weird"] {
            let f = fs::File::create(dl.join(n)).unwrap();
            if n != "new.pdf" { f.set_modified(old).unwrap(); }
        }
        let j = job(Kind::SortDownloads, &[("dir", &dl.display().to_string())]);
        // hide magpie-cli so the built-in sorter runs
        let p = c.home.join("s.sh");
        fs::write(&p, c.script(&j).replace("command -v magpie-cli", "false")).unwrap();
        assert!(Command::new("/bin/bash").arg(&p).env_remove("DBUS_SESSION_BUS_ADDRESS").status().unwrap().success());
        assert!(dl.join("Documents/a.PDF").exists());
        assert!(dl.join("Images/pic (1).png").exists() && dl.join("Images/pic.png").exists());
        for stays in ["new.pdf", "big.iso.part", "noext", "x.weird"] { assert!(dl.join(stays).exists(), "{stays}"); }
    }

    #[test]
    fn manual_jobs_are_not_scheduled() {
        let mut j = Job::new(Kind::Script);
        j.schedule = "manual".into();
        assert!(validate(&j).is_ok() && !scheduled(&j));
        assert_eq!(describe("0 2 * * *"), "Every night at 2 AM");
    }

    #[test]
    fn disk_alert_runs() {
        let (c, _) = ctx();
        let (rc, log) = run(&c, &job(Kind::DiskAlert, &[("mounts", "/")]));
        assert_eq!(rc, 0, "{log}");
        assert!(log.contains("% full"));
    }
}
