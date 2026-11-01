//! Cadence: set up bash scripts, cron jobs and rsync backups on this machine.
//! Examples: ready-made jobs in plain words, a question or two each. My jobs: every job, with the
//! full editor. Saving writes the job's script and both crontabs (user + /etc/cron.d) in one go.

mod jobs;
mod recipes;

use adw::prelude::*;
use gtk::glib;
use jobs::{Ctx, Input, Job, Kind, PLAIN};
use recipes::{Ask, Recipe};
use std::cell::{Cell, RefCell};
use std::rc::Rc;


enum W { Text(adw::EntryRow), Multi(gtk::TextView), Bool(adw::SwitchRow) }

struct Form {
    name: adw::EntryRow,
    fields: Vec<(&'static str, W)>,
    schedule: adw::EntryRow,
    enabled: adw::SwitchRow,
    root: adw::SwitchRow,
    notify: adw::SwitchRow,
    catch_up: adw::SwitchRow,
    ping: adw::EntryRow,
    log: gtk::TextView,
}

struct App {
    ctx: Ctx,
    jobs: RefCell<Vec<Job>>,
    sel: Cell<Option<usize>>,
    /// A job added but not saved yet: kept out of the config and crontabs until it is.
    fresh: RefCell<Option<String>>,
    list: gtk::ListBox,
    rows: RefCell<Vec<adw::ActionRow>>,
    quiet: Cell<bool>,
    editor: adw::Bin,
    toasts: adw::ToastOverlay,
    stack: adw::ViewStack,
    form: RefCell<Option<Form>>,
}

const HELP: &str = "Cadence: run backups, clean-ups and scripts on a schedule.

usage:
  cadence            open the app
  cadence --apply    rebuild the scripts and crontab from ~/.config/cadence/jobs.json
                     (after copying jobs.json to a new computer; asks for the admin
                     password if there are administrator jobs)
  cadence --list     list the jobs and when they last ran
  cadence --version";

fn main() -> glib::ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--apply") => {
            let ctx = Ctx::current();
            let jobs = jobs::load(&ctx.config());
            return match ctx.apply(&jobs) {
                Ok(m) => { println!("{m}"); glib::ExitCode::SUCCESS }
                Err(e) => { eprintln!("cadence: {e}"); glib::ExitCode::FAILURE }
            };
        }
        Some("--list") => {
            let ctx = Ctx::current();
            for j in jobs::load(&ctx.config()) {
                let last = match jobs::last_run(&ctx.log_path(&j)) {
                    Some((ts, rc)) => format!("{} exit {rc}", chrono::DateTime::from_timestamp(ts, 0).map(|t| t.with_timezone(&chrono::Local).format("%F %R").to_string()).unwrap_or_default()),
                    None => "never run".into(),
                };
                let state = if !j.enabled { "paused" } else { "on" };
                println!("{:<40} {:<28} {:<7} {last}", j.name, jobs::describe(&j.schedule), state);
            }
            return glib::ExitCode::SUCCESS;
        }
        Some("--version") => { println!("cadence {}", env!("CARGO_PKG_VERSION")); return glib::ExitCode::SUCCESS; }
        Some("--help" | "-h") => { println!("{HELP}"); return glib::ExitCode::SUCCESS; }
        Some(other) => { eprintln!("cadence: unknown option {other}\n\n{HELP}"); return glib::ExitCode::FAILURE; }
        None => {}
    }
    let app = adw::Application::builder().application_id("io.github.stolly345.cadence").build();
    app.connect_activate(build);
    app.run()
}

fn build(a: &adw::Application) {
    let ctx = Ctx::current();
    let jobs = jobs::load(&ctx.config());
    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    let st = Rc::new(App {
        ctx, jobs: RefCell::new(jobs), sel: Cell::new(None), fresh: RefCell::new(None), list: list.clone(),
        rows: RefCell::new(vec![]), quiet: Cell::new(false), editor: adw::Bin::new(),
        toasts: adw::ToastOverlay::new(), stack: adw::ViewStack::new(), form: RefCell::new(None),
    });

    let header = adw::HeaderBar::new();
    let add = gtk::MenuButton::builder().icon_name("list-add-symbolic").tooltip_text("New blank job (advanced)").build();
    let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
    for k in Kind::ALL {
        let b = gtk::Button::with_label(k.label());
        b.add_css_class("flat");
        let (st, add) = (st.clone(), add.clone());
        b.connect_clicked(move |_| { add.popdown(); new_job(&st, k); });
        menu.append(&b);
    }
    add.set_popover(Some(&gtk::Popover::builder().child(&menu).build()));
    header.pack_start(&add);

    let side = gtk::ScrolledWindow::builder().child(&list).width_request(300).hscrollbar_policy(gtk::PolicyType::Never).build();
    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&side));
    paned.set_end_child(Some(&st.editor));
    paned.set_shrink_start_child(false);
    paned.set_resize_start_child(false);
    st.stack.add_titled_with_icon(&gallery(&st), Some("examples"), "Examples", "starred-symbolic");
    st.stack.add_titled_with_icon(&paned, Some("jobs"), "My jobs", "alarm-symbolic");
    if !st.jobs.borrow().is_empty() { st.stack.set_visible_child_name("jobs"); }
    header.set_title_widget(Some(&adw::ViewSwitcher::builder().stack(&st.stack).policy(adw::ViewSwitcherPolicy::Wide).build()));
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    // Arch ships cron disabled, and some systems have none: without it nothing runs on schedule
    if !cron_running() {
        tv.add_top_bar(&adw::Banner::builder().revealed(true).title(
            "The cron service isn't running, so jobs won't run on their own. To fix it, run in a terminal: \
             sudo systemctl enable --now cronie   (on Ubuntu/Debian: cron)").build());
    }
    tv.set_content(Some(&st.stack));
    st.toasts.set_child(Some(&tv));

    {
        let st = st.clone();
        list.connect_row_selected(move |_, row| {
            if let Some(r) = row && !st.quiet.get() { show_job(&st, r.index() as usize); }
        });
    }
    {
        let st = st.clone();
        glib::timeout_add_seconds_local(5, move || { update_status(&st); glib::ControlFlow::Continue });
    }
    refresh_list(&st);
    placeholder(&st);
    adw::ApplicationWindow::builder().application(a).title("Cadence").default_width(1050).default_height(760)
        .content(&st.toasts).build().present();
}

fn toast(st: &App, msg: &str) { st.toasts.add_toast(adw::Toast::builder().title(msg).timeout(6).build()); }

fn placeholder(st: &App) {
    let p = adw::StatusPage::builder().icon_name("alarm-symbolic").title("My jobs")
        .description("Pick a job on the left. To add one, start from Examples, or press + for a blank job.").build();
    st.editor.set_child(Some(&p));
    st.form.replace(None);
}

fn status_text(st: &App, j: &Job) -> String {
    let when = |ts: i64| {
        let m = (chrono::Local::now().timestamp() - ts).max(0) / 60;
        if m < 1 { "just now".into() } else if m < 120 { format!("{m} min ago") } else if m < 2880 { format!("{} h ago", m / 60) } else { format!("{} days ago", m / 1440) }
    };
    let run = match jobs::last_run(&st.ctx.log_path(j)) {
        Some((ts, 0)) => format!("✓ ok {}", when(ts)),
        Some((ts, 75)) => format!("⏸ skipped {} (drive or NAS not connected)", when(ts)),
        Some((ts, rc)) => format!("✗ failed (exit {rc}) {}", when(ts)),
        None => "never run".into(),
    };
    let fresh = st.fresh.borrow().as_deref() == Some(&j.id);
    let paused = if fresh { " · not saved" } else if !j.enabled { " · paused" } else { "" };
    format!("{} · {}{}{paused}", jobs::describe(&j.schedule), run, if j.root { " · needs password" } else { "" })
}

fn refresh_list(st: &App) {
    st.quiet.set(true);
    st.list.remove_all();
    let mut rows = vec![];
    for j in st.jobs.borrow().iter() {
        let r = adw::ActionRow::builder().title(glib::markup_escape_text(&j.name)).subtitle(status_text(st, j)).build();
        st.list.append(&r);
        rows.push(r);
    }
    if let Some(i) = st.sel.get() && let Some(r) = st.list.row_at_index(i as i32) { st.list.select_row(Some(&r)); }
    st.rows.replace(rows);
    st.quiet.set(false);
}

fn update_status(st: &App) {
    for (r, j) in st.rows.borrow().iter().zip(st.jobs.borrow().iter()) { r.set_subtitle(&status_text(st, j)); }
    if let (Some(i), Some(f)) = (st.sel.get(), st.form.borrow().as_ref()) {
        let j = &st.jobs.borrow()[i];
        fill_log(&st.ctx, j, &f.log);
    }
}

fn fill_log(ctx: &Ctx, j: &Job, tv: &gtk::TextView) {
    let s = std::fs::read_to_string(ctx.log_path(j)).unwrap_or_else(|_| "No runs yet.".into());
    let lines: Vec<&str> = s.lines().collect();
    let tail = lines[lines.len().saturating_sub(300)..].join("\n");
    let b = tv.buffer();
    if b.text(&b.start_iter(), &b.end_iter(), false) != tail {
        b.set_text(&tail);
        let mut end = b.end_iter();
        tv.scroll_to_iter(&mut end, 0.0, false, 0.0, 1.0);
    }
}

fn new_job(st: &Rc<App>, k: Kind) {
    // only one unsaved job at a time: a second + replaces the first
    if let Some(old) = st.fresh.take() { st.jobs.borrow_mut().retain(|x| x.id != old); }
    let j = Job::new(k);
    st.fresh.replace(Some(j.id.clone()));
    st.jobs.borrow_mut().push(j);
    st.sel.set(Some(st.jobs.borrow().len() - 1));
    st.stack.set_visible_child_name("jobs");
    refresh_list(st);
    show_job(st, st.jobs.borrow().len() - 1);
}

fn mono(tv: &gtk::TextView, h: i32) -> gtk::ScrolledWindow {
    tv.set_monospace(true);
    tv.set_top_margin(8);
    tv.set_left_margin(8);
    tv.set_right_margin(8);
    tv.set_bottom_margin(8);
    let sw = gtk::ScrolledWindow::builder().child(tv).min_content_height(h).build();
    sw.add_css_class("card");
    sw
}

fn show_job(st: &Rc<App>, i: usize) {
    st.sel.set(Some(i));
    let j = st.jobs.borrow()[i].clone();
    let page = adw::PreferencesPage::new();

    // rows go in the current group; a text box (script, folder list) gets a group of its own,
    // so everything stays in field order (a group shows plain widgets after its rows)
    let mut g = adw::PreferencesGroup::builder().title(j.kind.label()).build();
    page.add(&g);
    let name = adw::EntryRow::builder().title("Name").text(&j.name).build();
    g.add(&name);
    let mut fields = vec![];
    for f in j.kind.fields() {
        let v = j.param(f);
        let w = match f.input {
            Input::Text => { let e = adw::EntryRow::builder().title(f.label).text(v).build(); g.add(&e); W::Text(e) }
            Input::Bool => { let s = adw::SwitchRow::builder().title(f.label).active(v == "yes").build(); g.add(&s); W::Bool(s) }
            Input::Secret => {
                let e = adw::PasswordEntryRow::builder().title(f.label).text(v).build();
                g.add(&e);
                W::Text(e.upcast())
            }
            Input::Multi | Input::List => {
                let tv = gtk::TextView::new();
                tv.buffer().set_text(v);
                let tg = adw::PreferencesGroup::builder().title(f.label).build();
                if f.key == "src" {
                    let add = gtk::Button::builder().label("Add folder…").valign(gtk::Align::Center).build();
                    let (st, tv) = (st.clone(), tv.clone());
                    add.connect_clicked(move |_| {
                        let tv = tv.clone();
                        pick_folder(&st, "Add a folder", None, move |p| {
                            let b = tv.buffer();
                            let cur = b.text(&b.start_iter(), &b.end_iter(), false);
                            let sep = if cur.is_empty() || cur.ends_with('\n') { "" } else { "\n" };
                            b.insert(&mut b.end_iter(), &format!("{sep}{p}\n"));
                        });
                    });
                    tg.set_header_suffix(Some(&add));
                }
                tg.add(&mono(&tv, if f.input == Input::List { 90 } else { 220 }));
                page.add(&tg);
                g = adw::PreferencesGroup::new();
                page.add(&g);
                W::Multi(tv)
            }
        };
        fields.push((f.key, w));
    }

    let sg = adw::PreferencesGroup::builder().title("When").build();
    let (how, schedule) = schedule_rows(&j.schedule);
    sg.add(&how);
    let adv = adw::ExpanderRow::builder().title("Advanced: cron schedule").build();
    adv.add_row(&schedule);
    sg.add(&adv);
    let next = gtk::Label::builder().xalign(0.0).wrap(true).margin_top(6).build();
    next.add_css_class("dim-label");
    let show_next = {
        let next = next.clone();
        move |s: &str| next.set_label(&match jobs::next_runs(s, 4) {
            Ok(_) if s.trim() == "manual" => "Runs only when you press Run now.".into(),
            Ok(v) if v.is_empty() => "Runs each time the computer starts.".into(),
            Ok(v) => format!("Next: {}", v.iter().map(|t| t.format("%a %d %b %H:%M").to_string()).collect::<Vec<_>>().join(" · ")),
            Err(e) => format!("⚠ {e}"),
        })
    };
    show_next(&j.schedule);
    schedule.connect_changed(move |e| show_next(&e.text()));
    sg.add(&next);
    let note = gtk::Label::builder().xalign(0.0).wrap(true).margin_top(4)
        .label("Jobs can't run while the computer is asleep or off. With “Make up missed runs” on (below), a missed run happens within the hour after it's back on.").build();
    note.add_css_class("dim-label");
    note.add_css_class("caption");
    sg.add(&note);
    page.add(&sg);

    let og = adw::PreferencesGroup::builder().title("Options").build();
    let enabled = adw::SwitchRow::builder().title("Enabled").active(j.enabled).build();
    let root = adw::SwitchRow::builder().title("Run as administrator (root)").subtitle("Asks for your password when saved")
        .active(j.root).sensitive(j.kind.forced_root().is_none()).build();
    let notify = adw::SwitchRow::builder().title("Notify when it succeeds").subtitle("Failures always notify").active(j.notify_success).build();
    let catch_up = adw::SwitchRow::builder().title("Make up missed runs")
        .subtitle("If the computer was asleep or off at the scheduled time, run within the hour once it's back on")
        .active(j.catch_up).build();
    let ping = adw::EntryRow::builder().title("Ping URL after each run (Healthchecks.io etc., optional)").text(&j.ping_url).build();
    for w in [enabled.upcast_ref::<gtk::Widget>(), catch_up.upcast_ref(), root.upcast_ref(), notify.upcast_ref(), ping.upcast_ref()] { og.add(w); }
    page.add(&og);

    let bar = gtk::Box::builder().spacing(8).margin_top(4).build();
    let save = gtk::Button::with_label("Save and schedule");
    save.add_css_class("suggested-action");
    let run = gtk::Button::with_label("Run now");
    let view = gtk::Button::with_label("View script");
    let del = gtk::Button::with_label("Delete");
    del.add_css_class("destructive-action");
    del.set_hexpand(true);
    del.set_halign(gtk::Align::End);
    for b in [&save, &run, &view, &del] { bar.append(b); }
    let bg = adw::PreferencesGroup::new();
    bg.add(&bar);
    page.add(&bg);

    let lg = adw::PreferencesGroup::builder().title("Recent runs").description(st.ctx.log_path(&j).display().to_string()).build();
    let log = gtk::TextView::builder().editable(false).cursor_visible(false).wrap_mode(gtk::WrapMode::WordChar).build();
    lg.add(&mono(&log, 260));
    page.add(&lg);
    fill_log(&st.ctx, &j, &log);

    st.form.replace(Some(Form { name, fields, schedule, enabled, root, notify, catch_up, ping, log }));
    st.editor.set_child(Some(&page));

    { let st = st.clone(); save.connect_clicked(move |_| { save_current(&st); }); }
    {
        let st = st.clone();
        run.connect_clicked(move |_| {
            if !save_current(&st) { return; }
            let j = st.jobs.borrow()[st.sel.get().unwrap()].clone();
            let script = st.ctx.script_path(&j);
            run_job(&st, &j, &script);
        });
    }
    {
        let st = st.clone();
        view.connect_clicked(move |_| {
            let Some(j) = read_form(&st) else { return };
            let tv = gtk::TextView::builder().editable(false).build();
            tv.buffer().set_text(&st.ctx.script(&j));
            let tb = adw::ToolbarView::new();
            tb.add_top_bar(&adw::HeaderBar::new());
            tb.set_content(Some(&mono(&tv, 500)));
            let d = adw::Dialog::builder().title(st.ctx.script_path(&j).display().to_string()).content_width(860).content_height(640).child(&tb).build();
            d.present(Some(&st.toasts));
        });
    }
    {
        let st = st.clone();
        del.connect_clicked(move |_| {
            let name = st.jobs.borrow()[st.sel.get().unwrap()].name.clone();
            let d = adw::AlertDialog::new(Some(&format!("Delete “{name}”?")), Some("Its schedule and script are removed. Its log is kept."));
            d.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
            d.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            let st2 = st.clone();
            d.connect_response(Some("delete"), move |_, _| {
                let Some(i) = st2.sel.take() else { return };
                st2.jobs.borrow_mut().remove(i);
                persist(&st2);
                refresh_list(&st2);
                placeholder(&st2);
            });
            d.present(Some(&st.toasts));
        });
    }
}

/// The selected job with the form's values in it.
fn read_form(st: &App) -> Option<Job> {
    let form = st.form.borrow();
    let f = form.as_ref()?;
    let mut j = st.jobs.borrow()[st.sel.get()?].clone();
    j.name = f.name.text().trim().to_string();
    for (k, w) in &f.fields {
        let v = match w {
            W::Text(e) => e.text().trim().to_string(),
            W::Bool(s) => if s.is_active() { "yes".into() } else { "no".into() },
            W::Multi(tv) => { let b = tv.buffer(); b.text(&b.start_iter(), &b.end_iter(), false).to_string() }
        };
        j.params.insert(k.to_string(), v);
    }
    j.schedule = f.schedule.text().trim().to_string();
    j.enabled = f.enabled.is_active();
    j.root = j.kind.forced_root().unwrap_or(f.root.is_active());
    j.notify_success = f.notify.is_active();
    j.catch_up = f.catch_up.is_active();
    j.ping_url = f.ping.text().trim().to_string();
    Some(j)
}

fn save_current(st: &App) -> bool {
    let Some(j) = read_form(st) else { return false };
    if let Err(e) = jobs::validate(&j) { toast(st, &e); return false; }
    let i = st.sel.get().unwrap();
    st.jobs.borrow_mut()[i] = j;
    if st.fresh.borrow().as_deref() == Some(&st.jobs.borrow()[i].id) { st.fresh.replace(None); }
    let ok = persist(st);
    refresh_list(st);
    ok
}

/// Save the config and write scripts + crontabs. Unsaved fresh jobs are kept out of both.
fn persist(st: &App) -> bool {
    let fresh = st.fresh.borrow().clone();
    let jobs: Vec<Job> = st.jobs.borrow().iter().filter(|j| Some(&j.id) != fresh.as_ref()).cloned().collect();
    if let Err(e) = jobs::save(&st.ctx.config(), &jobs) { toast(st, &format!("Couldn't save: {e}")); return false; }
    match st.ctx.apply(&jobs) {
        Ok(msg) => { toast(st, &format!("Saved: {msg}")); true }
        Err(e) => { toast(st, &e); false }
    }
}

/// "How often" in words, kept in step with the cron entry (under Advanced) that it fills in.
fn schedule_rows(current: &str) -> (adw::ComboRow, adw::EntryRow) {
    let mut names: Vec<&str> = PLAIN.iter().map(|(n, _)| *n).collect();
    names.push("Custom (see Advanced)");
    let pos = |s: &str| PLAIN.iter().position(|(_, c)| *c == s.trim()).unwrap_or(PLAIN.len()) as u32;
    let how = adw::ComboRow::builder().title("How often").model(&gtk::StringList::new(&names)).selected(pos(current)).build();
    let cron = adw::EntryRow::builder().title("Cron schedule (minute hour day month weekday)").text(current).build();
    {
        let cron = cron.clone();
        how.connect_selected_notify(move |c| {
            if let Some((_, s)) = PLAIN.get(c.selected() as usize) && cron.text() != *s { cron.set_text(s); }
        });
    }
    { let how = how.clone(); cron.connect_changed(move |e| how.set_selected(pos(&e.text()))); }
    (how, cron)
}

fn window(st: &App) -> Option<gtk::Window> { st.toasts.root().and_downcast::<gtk::Window>() }

/// Run a job's script by hand, then show what it printed.
fn run_job(st: &Rc<App>, j: &Job, script: &std::path::Path) {
    let argv = Ctx::run_argv(j, script);
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(|a| a.as_os_str()).collect();
    let p = match gtk::gio::Subprocess::newv(&argv, gtk::gio::SubprocessFlags::NONE) {
        Ok(p) => p,
        Err(e) => return toast(st, &format!("Couldn't start it: {e}")),
    };
    toast(st, &format!("Running “{}”…", j.name));
    let (st, j) = (st.clone(), j.clone());
    p.clone().wait_async(None::<&gtk::gio::Cancellable>, move |_| {
        update_status(&st);
        let code = if p.has_exited() { p.exit_status() } else { -1 };
        let out = jobs::last_output(&st.ctx.log_path(&j));
        let (title, body) = match code {
            0 => ("✓ Finished".to_string(), out),
            75 => ("⏸ Skipped for now".to_string(), out),
            126 | 127 if j.root => ("Not run".to_string(), "The password wasn't given, so nothing was changed.".to_string()),
            c => (format!("✗ Didn't work (exit {c})"), out),
        };
        let tv = gtk::TextView::builder().editable(false).cursor_visible(false).wrap_mode(gtk::WrapMode::WordChar).build();
        tv.buffer().set_text(if body.trim().is_empty() { "(it printed nothing)" } else { &body });
        let tb = adw::ToolbarView::new();
        tb.add_top_bar(&adw::HeaderBar::new());
        let page = gtk::Box::builder().orientation(gtk::Orientation::Vertical).margin_start(12).margin_end(12).margin_bottom(12).build();
        page.append(&mono(&tv, 380));
        tb.set_content(Some(&page));
        let d = adw::Dialog::builder().title(format!("{}: {title}", j.name)).content_width(760).content_height(520).child(&tb).build();
        d.present(Some(&st.toasts));
    });
}

/// The Examples page: every recipe, by category.
fn gallery(st: &Rc<App>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder().description(
        "Ready-made jobs. Pick one, answer a question or two, and Cadence does the rest: \
         it runs on its own from then on and tells you if something goes wrong.").build();
    for cat in recipes::CATEGORIES {
        let g = adw::PreferencesGroup::builder().title(*cat).build();
        for r in recipes::RECIPES.iter().filter(|r| r.category == *cat) {
            let row = adw::ActionRow::builder().title(r.title).subtitle(r.blurb).activatable(true).subtitle_lines(3).build();
            row.add_prefix(&gtk::Image::from_icon_name(r.icon));
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let st = st.clone();
            row.connect_activated(move |_| open_recipe(&st, r));
            g.add(&row);
        }
        page.add(&g);
    }
    page
}

/// One example: its questions, how often, and Run now / Turn on.
fn open_recipe(st: &Rc<App>, r: &'static Recipe) {
    let page = adw::PreferencesPage::new();
    // a folder list gets its own group, so questions stay in order (see show_job)
    let mut g = adw::PreferencesGroup::builder().title(r.title).description(r.blurb).build();
    page.add(&g);
    let answers: Vec<Rc<RefCell<String>>> = r.asks.iter().map(|_| Rc::new(RefCell::new(String::new()))).collect();
    for (a, ans) in r.asks.iter().zip(&answers) {
        match *a {
            Ask::Folder { key, label } => {
                let start = r.params.iter().find(|(k, _)| *k == key).map(|(_, v)| v.replacen('~', &st.ctx.home.display().to_string(), 1));
                ans.replace(start.clone().unwrap_or_default());
                let row = adw::ActionRow::builder().title(label).subtitle(start.unwrap_or_else(|| "Not chosen yet".into())).build();
                let pick = gtk::Button::builder().label("Choose…").valign(gtk::Align::Center).build();
                let (row2, ans, st2) = (row.clone(), ans.clone(), st.clone());
                pick.connect_clicked(move |_| {
                    let d = gtk::FileDialog::builder().title(label).build();
                    if !ans.borrow().is_empty() { d.set_initial_folder(Some(&gtk::gio::File::for_path(&*ans.borrow()))); }
                    let (row, ans) = (row2.clone(), ans.clone());
                    d.select_folder(window(&st2).as_ref(), None::<&gtk::gio::Cancellable>, move |res| {
                        if let Ok(f) = res && let Some(p) = f.path() {
                            let p = p.display().to_string();
                            row.set_subtitle(&p);
                            ans.replace(p);
                        }
                    });
                });
                row.add_suffix(&pick);
                g.add(&row);
            }
            Ask::Folders { label } => {
                let start: Vec<String> = r.params.iter().find(|(k, _)| *k == "src")
                    .map(|(_, v)| jobs::list(v).iter().map(|x| x.replacen('~', &st.ctx.home.display().to_string(), 1)).collect())
                    .unwrap_or_default();
                let lb = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::None).build();
                lb.add_css_class("boxed-list");
                let tg = adw::PreferencesGroup::builder().title(label).build();
                tg.add(&lb);
                page.add(&tg);
                g = adw::PreferencesGroup::new();
                page.add(&g);
                render_folders(st, &lb, Rc::new(RefCell::new(start)), ans.clone());
            }
            Ask::Password { label } => {
                let e = adw::PasswordEntryRow::builder().title(format!("{label} (leave blank if none)")).build();
                let ans = ans.clone();
                e.connect_changed(move |e| { ans.replace(e.text().to_string()); });
                g.add(&e);
            }
            Ask::Text { key, label, example } => {
                let pre = r.params.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string()).unwrap_or_default();
                ans.replace(pre.clone());
                let e = adw::EntryRow::builder().title(format!("{label} (e.g. {example})")).text(pre).build();
                let ans = ans.clone();
                e.connect_changed(move |e| { ans.replace(e.text().to_string()); });
                g.add(&e);
            }
            Ask::Drive { label, .. } => {
                let drives = Rc::new(RefCell::new(recipes::drives()));
                let combo = adw::ComboRow::builder().title(label).build();
                let fill = {
                    let (combo, drives, ans) = (combo.clone(), drives.clone(), ans.clone());
                    move || {
                        let names: Vec<String> = if drives.borrow().is_empty() {
                            vec!["No drive plugged in".into()]
                        } else { drives.borrow().iter().map(|(n, m)| format!("{n}   ({m})")).collect() };
                        let names: Vec<&str> = names.iter().map(String::as_str).collect();
                        combo.set_model(Some(&gtk::StringList::new(&names)));
                        combo.set_selected(0);
                        ans.replace(drives.borrow().first().map(|d| d.1.clone()).unwrap_or_default());
                    }
                };
                fill();
                {
                    let (drives, ans) = (drives.clone(), ans.clone());
                    combo.connect_selected_notify(move |c| {
                        if let Some(d) = drives.borrow().get(c.selected() as usize) { ans.replace(d.1.clone()); }
                    });
                }
                let refresh = gtk::Button::builder().icon_name("view-refresh-symbolic").tooltip_text("Plugged one in? Look again").valign(gtk::Align::Center).build();
                refresh.add_css_class("flat");
                let other = gtk::Button::builder().label("Other folder…").valign(gtk::Align::Center).build();
                {
                    let (drives, fill) = (drives.clone(), fill.clone());
                    refresh.connect_clicked(move |_| { drives.replace(recipes::drives()); fill(); });
                }
                {
                    let (drives, combo, st2) = (drives.clone(), combo.clone(), st.clone());
                    other.connect_clicked(move |_| {
                        let d = gtk::FileDialog::builder().title(label).build();
                        let (drives, combo, fill) = (drives.clone(), combo.clone(), fill.clone());
                        d.select_folder(window(&st2).as_ref(), None::<&gtk::gio::Cancellable>, move |res| {
                            if let Ok(f) = res && let Some(p) = f.path() {
                                let p = p.display().to_string();
                                drives.borrow_mut().insert(0, (p.rsplit('/').next().unwrap_or(&p).to_string(), p));
                                fill();
                                combo.set_selected(0);
                            }
                        });
                    });
                }
                combo.add_suffix(&refresh);
                combo.add_suffix(&other);
                g.add(&combo);
            }
        }
    }

    let wg = adw::PreferencesGroup::new();
    let (how, cron) = schedule_rows(r.schedule);
    wg.add(&how);
    if r.root || r.kind.forced_root() == Some(true) {
        wg.add(&adw::ActionRow::builder().title("Needs your password").subtitle("This changes system files, so Linux asks for your password to allow it.").build());
    }
    page.add(&wg);

    let bar = gtk::Box::builder().spacing(8).halign(gtk::Align::End).build();
    let try_it = gtk::Button::with_label("Run now");
    let on = gtk::Button::with_label("Turn on");
    on.add_css_class("suggested-action");
    bar.append(&try_it);
    bar.append(&on);
    let label = |sel: u32| if PLAIN.get(sel as usize).is_some_and(|p| p.1 == "manual") { "Save to My jobs" } else { "Turn on" };
    on.set_label(label(how.selected()));
    { let on = on.clone(); how.connect_selected_notify(move |c| on.set_label(label(c.selected()))); }
    let bg = adw::PreferencesGroup::new();
    bg.add(&bar);
    page.add(&bg);

    let tb = adw::ToolbarView::new();
    tb.add_top_bar(&adw::HeaderBar::new());
    tb.set_content(Some(&page));
    let d = adw::Dialog::builder().title(r.title).content_width(640).content_height(620).child(&tb).build();

    let build = {
        let (answers, cron) = (answers.clone(), cron.clone());
        move || r.job(&answers.iter().map(|a| a.borrow().clone()).collect::<Vec<_>>(), cron.text().as_str())
    };
    {
        let (st, build) = (st.clone(), build.clone());
        try_it.connect_clicked(move |_| {
            let mut j = match build() { Ok(j) => j, Err(e) => return toast(&st, &e) };
            // a trial run: its own id, so it never shares a log or lock with a saved job
            j.id = format!("try-{}", recipes::RECIPES.iter().position(|x| std::ptr::eq(x, r)).unwrap_or(0));
            let dir = st.ctx.home.join(".cache/cadence/run");
            let path = dir.join(format!("{}.sh", j.id));
            if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, st.ctx.script(&j))) {
                return toast(&st, &e.to_string());
            }
            if let Err(e) = st.ctx.write_secret(&j) { return toast(&st, &e); }
            run_job(&st, &j, &path);
        });
    }
    {
        let (st, d2) = (st.clone(), d.clone());
        on.connect_clicked(move |_| {
            let j = match build() { Ok(j) => j, Err(e) => return toast(&st, &e) };
            st.jobs.borrow_mut().push(j);
            // kept even if scheduling failed (e.g. password refused): Save in My jobs retries
            persist(&st);
            st.sel.set(Some(st.jobs.borrow().len() - 1));
            refresh_list(&st);
            show_job(&st, st.jobs.borrow().len() - 1);
            st.stack.set_visible_child_name("jobs");
            d2.close();
        });
    }
    d.present(Some(&st.toasts));
}

/// A cron daemon is running (Debian's is `cron`, Fedora's and Arch's cronie is `crond`).
fn cron_running() -> bool {
    std::fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| {
        std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| matches!(c.trim(), "cron" | "crond" | "fcron" | "dcron"))
    })
}

/// Ask for a folder, then hand its path to `done`.
fn pick_folder(st: &App, title: &str, start: Option<&str>, done: impl FnOnce(String) + 'static) {
    let d = gtk::FileDialog::builder().title(title).build();
    if let Some(s) = start.filter(|s| !s.is_empty()) { d.set_initial_folder(Some(&gtk::gio::File::for_path(s))); }
    d.select_folder(window(st).as_ref(), None::<&gtk::gio::Cancellable>, move |res| {
        if let Ok(f) = res && let Some(p) = f.path() { done(p.display().to_string()); }
    });
}

/// The folder list in an example: one row per folder with a remove button, then "Add folder…".
/// The answer is kept as one path per line.
fn render_folders(st: &Rc<App>, lb: &gtk::ListBox, folders: Rc<RefCell<Vec<String>>>, ans: Rc<RefCell<String>>) {
    ans.replace(folders.borrow().join("\n"));
    lb.remove_all();
    for (i, f) in folders.borrow().iter().enumerate() {
        let row = adw::ActionRow::builder().title(glib::markup_escape_text(f.rsplit('/').next().unwrap_or(f))).subtitle(glib::markup_escape_text(f)).build();
        row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
        let rm = gtk::Button::builder().icon_name("list-remove-symbolic").tooltip_text("Don't back up this folder").valign(gtk::Align::Center).build();
        rm.add_css_class("flat");
        let (st, lb2, folders2, ans) = (st.clone(), lb.clone(), folders.clone(), ans.clone());
        rm.connect_clicked(move |_| {
            folders2.borrow_mut().remove(i);
            render_folders(&st, &lb2, folders2.clone(), ans.clone());
        });
        row.add_suffix(&rm);
        lb.append(&row);
    }
    let add = adw::ActionRow::builder().title("Add folder…").activatable(true).build();
    add.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
    let (st, lb2, ans) = (st.clone(), lb.clone(), ans.clone());
    add.connect_activated(move |_| {
        let (st2, lb3, folders, ans) = (st.clone(), lb2.clone(), folders.clone(), ans.clone());
        let home = st.ctx.home.display().to_string();
        pick_folder(&st, "Add a folder to back up", Some(&home), move |p| {
            if !folders.borrow().contains(&p) { folders.borrow_mut().push(p); }
            render_folders(&st2, &lb3, folders, ans);
        });
    });
    lb.append(&add);
}
