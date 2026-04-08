// Copyright 2026 Mark Mandel
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, EventControllerKey,
    GestureClick, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType, Revealer,
    RevealerTransitionType, ScrolledWindow, SelectionMode, ToggleButton,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::db::{self, Notification};
use crate::Result;

// ---------------------------------------------------------------------------
// Rose Pine palette — matches dotfiles/quickshell/Common/Theme.qml
// ---------------------------------------------------------------------------
const CSS: &str = r#"
/* Rose Pine Main
 * Semantic roles per https://rosepinetheme.com/palette/
 *   base       = primary background (app frames, header)
 *   surface    = secondary background (cards, inputs)
 *   overlay    = tertiary background (popovers, notifications, dialogs)
 *   muted      = disabled / unfocused foreground (timestamps)
 *   subtle     = comments / secondary foreground (body text)
 *   text       = normal foreground (titles, active content)
 *   love       = errors / unread indicator
 *   gold       = warnings / pinned indicator
 *   pine       = active / selected state (toggle on)
 *   foam       = information / additions (read button)
 *   iris       = hints / links (header accent)
 *   highlight* = interactive backgrounds
 */
@define-color base           #191724;
@define-color surface        #1f1d2e;
@define-color overlay        #26233a;
@define-color muted          #6e6a86;
@define-color subtle         #908caa;
@define-color rp_text        #e0def4;
@define-color love           #eb6f92;
@define-color gold           #f6c177;
@define-color rose           #ebbcba;
@define-color pine           #31748f;
@define-color foam           #9ccfd8;
@define-color iris           #c4a7e7;
@define-color highlight_low  #21202e;
@define-color highlight_med  #403d52;
@define-color highlight_high #524f67;

/* Panel — base is the primary background */
window {
    background-color: @base;
    color: @rp_text;
    font-family: "JetBrains Mono", monospace;
    font-size: 10pt;
}

/* Header sits on base, just needs a separator */
.notification-header {
    background-color: @base;
    border-bottom: 1px solid @highlight_med;
    padding: 10px 14px;
}

/* Iris = hints/links — a nice accent for the panel title */
.notification-header-title {
    font-weight: bold;
    font-size: 11pt;
    color: @iris;
    letter-spacing: 0.5px;
}

/* Header action buttons (Mark all read, etc.) */
.header-btn {
    border: 1px solid @highlight_med;
    border-radius: 12px;
    padding: 2px 10px;
    color: @subtle;
    font-size: 9pt;
}

.header-btn:hover {
    border-color: @highlight_high;
    color: @rp_text;
    background-color: @highlight_low;
}

/* "Show all" toggle — pine = active/selected state */
button.toggle {
    border: 1px solid @highlight_med;
    border-radius: 12px;
    padding: 2px 12px;
    color: @subtle;
    font-size: 9pt;
}

button.toggle:hover {
    border-color: @highlight_high;
    color: @rp_text;
    background-color: @highlight_low;
}

button.toggle:checked {
    background-color: @pine;
    border-color: @pine;
    color: @base;
    font-weight: bold;
}

/* Notification rows — overlay = tertiary background (notifications/dialogs) */
row.notification-row {
    background-color: @overlay;
    margin: 4px 8px;
    border-radius: 8px;
    border: 1px solid @highlight_med;
    padding: 0;
}

row.notification-row:hover {
    background-color: @highlight_med;
    border-color: @highlight_high;
}

/* Love = unread indicator (left accent border) */
row.notification-row.unread {
    border-left: 3px solid @love;
}

/* Gold = pinned/important (left accent border, overrides unread) */
row.notification-row.pinned-row {
    border-left: 3px solid @gold;
}

.notification-title {
    font-weight: bold;
    color: @rp_text;
}

/* Subtle = secondary foreground / comments */
.notification-body {
    color: @subtle;
    font-size: 9pt;
}

/* Muted = disabled / unfocused elements */
.notification-time {
    color: @muted;
    font-size: 8pt;
}

/* Action buttons */
button {
    background: transparent;
    border: none;
    color: @subtle;
    padding: 4px 8px;
    border-radius: 6px;
    min-height: 0;
    min-width: 0;
    font-size: 12pt;
}

button:hover {
    background-color: @highlight_high;
    color: @rp_text;
}

/* Gold = warnings / important-to-keep */
.pin-btn.active-pin {
    color: @gold;
}

/* Foam = information / additions */
.read-btn {
    color: @foam;
}

scrollbar slider {
    background-color: @highlight_med;
    border-radius: 4px;
    min-width: 4px;
    min-height: 4px;
}

scrollbar slider:hover {
    background-color: @highlight_high;
}

#notification-list,
#notification-list > row,
scrolledwindow,
scrolledwindow > viewport,
viewport {
    background-color: @base;
    color: @rp_text;
}
"#;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run() -> Result<()> {
    let conn = Rc::new(db::open_db()?);

    let app = Application::builder()
        .application_id("com.example.discord-notification-center")
        .build();

    app.connect_activate(move |app| {
        build_ui(app, conn.clone());
    });

    // Pass no args so GTK doesn't try to parse our clap args.
    app.run_with_args::<&str>(&[]);
    Ok(())
}

// ---------------------------------------------------------------------------
// Window construction
// ---------------------------------------------------------------------------

fn build_ui(app: &Application, conn: Rc<rusqlite::Connection>) {
    let provider = CssProvider::new();
    provider.load_from_data(CSS);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("no display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let window = ApplicationWindow::new(app);
    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Bottom, true);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_default_size(400, -1);

    // Esc closes the panel
    let key_ctrl = EventControllerKey::new();
    let win_weak = window.downgrade();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            if let Some(w) = win_weak.upgrade() {
                w.close();
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key_ctrl);

    // Header bar (plain Box to avoid window-decoration chrome)
    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.add_css_class("notification-header");

    let mark_all_btn = Button::with_label("✓ all");
    mark_all_btn.set_tooltip_text(Some("Mark all as read"));
    mark_all_btn.add_css_class("header-btn");
    header.append(&mark_all_btn);

    let title_lbl = Label::new(Some("Notifications"));
    title_lbl.set_hexpand(true);
    title_lbl.set_halign(gtk4::Align::Center);
    title_lbl.add_css_class("notification-header-title");
    header.append(&title_lbl);

    let show_all_btn = ToggleButton::with_label("All");
    header.append(&show_all_btn);

    // Notification list
    let list_box = ListBox::new();
    list_box.set_widget_name("notification-list");
    list_box.set_selection_mode(SelectionMode::None);

    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&list_box));

    let content = GtkBox::new(Orientation::Vertical, 0);
    content.append(&header);
    content.append(&scrolled);

    // Revealer provides the slide-in-from-right animation
    let revealer = Revealer::new();
    revealer.set_transition_type(RevealerTransitionType::SlideLeft);
    revealer.set_transition_duration(250);
    revealer.set_child(Some(&content));
    revealer.set_reveal_child(false);

    window.set_child(Some(&revealer));

    // Shared state
    let show_all = Rc::new(Cell::new(false));
    let last_id = Rc::new(Cell::new(0i64));

    load_notifications(&conn, &list_box, &show_all, &last_id);

    // Show-all toggle: clear list and reload
    {
        let conn = conn.clone();
        let list_box = list_box.clone();
        let show_all = show_all.clone();
        let last_id = last_id.clone();
        show_all_btn.connect_toggled(move |btn| {
            show_all.set(btn.is_active());
            while let Some(child) = list_box.first_child() {
                list_box.remove(&child);
            }
            load_notifications(&conn, &list_box, &show_all, &last_id);
        });
    }

    // Mark all read
    {
        let conn = conn.clone();
        let list_box = list_box.clone();
        let show_all = show_all.clone();
        let last_id = last_id.clone();
        mark_all_btn.connect_clicked(move |_| {
            let _ = db::mark_all_read(&*conn);
            while let Some(child) = list_box.first_child() {
                list_box.remove(&child);
            }
            load_notifications(&conn, &list_box, &show_all, &last_id);
        });
    }

    // 1-second poll for new notifications (skipped in show-all mode)
    {
        let conn = conn.clone();
        let list_box = list_box.clone();
        let show_all = show_all.clone();
        let last_id = last_id.clone();
        glib::timeout_add_local(Duration::from_secs(1), move || {
            if show_all.get() {
                return glib::ControlFlow::Continue;
            }
            if let Ok(new) = db::fetch_new_since(&*conn, last_id.get()) {
                for n in &new {
                    list_box.prepend(&build_row(n, &conn, &list_box, &show_all));
                    last_id.set(n.id);
                }
            }
            glib::ControlFlow::Continue
        });
    }

    window.present();
    // Trigger slide-in after window is shown
    revealer.set_reveal_child(true);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_notifications(
    conn: &Rc<rusqlite::Connection>,
    list_box: &ListBox,
    show_all: &Rc<Cell<bool>>,
    last_id: &Rc<Cell<i64>>,
) {
    let notifications = db::fetch_display(&**conn, show_all.get()).unwrap_or_default();
    if let Some(max) = notifications.iter().map(|n| n.id).max() {
        last_id.set(max);
    }
    for n in &notifications {
        list_box.append(&build_row(n, conn, list_box, show_all));
    }
}

fn build_row(
    n: &Notification,
    conn: &Rc<rusqlite::Connection>,
    list_box: &ListBox,
    show_all: &Rc<Cell<bool>>,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    row.add_css_class("notification-row");
    if !n.read   { row.add_css_class("unread"); }
    if n.pinned  { row.add_css_class("pinned-row"); }

    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(8);
    hbox.set_margin_bottom(8);
    hbox.set_margin_start(8);
    hbox.set_margin_end(8);

    // Icon (placeholder — future: fetch icon_url asynchronously)
    let icon = Image::from_icon_name("user-info-symbolic");
    icon.set_pixel_size(40);
    icon.set_valign(gtk4::Align::Start);
    hbox.append(&icon);

    // Text column
    let vbox = GtkBox::new(Orientation::Vertical, 2);
    vbox.set_hexpand(true);
    vbox.set_valign(gtk4::Align::Center);

    let title_lbl = Label::new(Some(&n.title));
    title_lbl.set_halign(gtk4::Align::Start);
    title_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    title_lbl.add_css_class("notification-title");

    let body_lbl = Label::new(Some(&n.body));
    body_lbl.set_halign(gtk4::Align::Start);
    body_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    body_lbl.add_css_class("notification-body");

    let time_lbl = Label::new(Some(&format_timestamp(n.message_timestamp.as_deref())));
    time_lbl.set_halign(gtk4::Align::Start);
    time_lbl.add_css_class("notification-time");

    vbox.append(&title_lbl);
    vbox.append(&body_lbl);
    vbox.append(&time_lbl);
    hbox.append(&vbox);

    // Action buttons
    let btn_box = GtkBox::new(Orientation::Vertical, 4);
    btn_box.set_valign(gtk4::Align::Center);

    let pin_btn = Button::with_label(if n.pinned { "📍" } else { "📌" });
    pin_btn.set_tooltip_text(Some(if n.pinned { "Unpin" } else { "Pin" }));
    pin_btn.add_css_class("pin-btn");
    if n.pinned { pin_btn.add_css_class("active-pin"); }

    let read_btn = Button::with_label(if n.read { "↩" } else { "✓" });
    read_btn.set_tooltip_text(Some(if n.read { "Mark unread" } else { "Mark read" }));
    read_btn.add_css_class("read-btn");

    btn_box.append(&pin_btn);
    btn_box.append(&read_btn);
    hbox.append(&btn_box);

    row.set_child(Some(&hbox));

    // Per-row mutable state (avoids reference cycles via closure params)
    let n_id = n.id;
    let pinned_state = Rc::new(Cell::new(n.pinned));
    let read_state = Rc::new(Cell::new(n.read));
    let url = discord_url(n);

    // Pin / unpin
    {
        let conn = conn.clone();
        let list_box = list_box.clone();
        let show_all = show_all.clone();
        let row_weak = row.downgrade();
        let pinned_state = pinned_state.clone();
        let read_state = read_state.clone();
        pin_btn.connect_clicked(move |btn| {
            let new_pinned = !pinned_state.get();
            pinned_state.set(new_pinned);
            let _ = db::set_pinned(&*conn, n_id, new_pinned);

            btn.set_label(if new_pinned { "📍" } else { "📌" });
            btn.set_tooltip_text(Some(if new_pinned { "Unpin" } else { "Pin" }));
            if new_pinned {
                btn.add_css_class("active-pin");
                if let Some(row) = row_weak.upgrade() { row.add_css_class("pinned-row"); }
            } else {
                btn.remove_css_class("active-pin");
                if let Some(row) = row_weak.upgrade() {
                    row.remove_css_class("pinned-row");
                    // Unpinned + already read → remove from default view
                    if !show_all.get() && read_state.get() {
                        list_box.remove(&row);
                    }
                }
            }
        });
    }

    // Mark read / unread
    {
        let conn = conn.clone();
        let list_box = list_box.clone();
        let show_all = show_all.clone();
        let row_weak = row.downgrade();
        let read_state = read_state.clone();
        let pinned_state = pinned_state.clone();
        read_btn.connect_clicked(move |btn| {
            let new_read = !read_state.get();
            read_state.set(new_read);
            let _ = db::set_read(&*conn, n_id, new_read);

            btn.set_label(if new_read { "↩" } else { "✓" });
            btn.set_tooltip_text(Some(if new_read { "Mark unread" } else { "Mark read" }));

            if let Some(row) = row_weak.upgrade() {
                if new_read {
                    row.remove_css_class("unread");
                    // Default mode: remove row unless pinned
                    if !show_all.get() && !pinned_state.get() {
                        list_box.remove(&row);
                    }
                } else {
                    row.add_css_class("unread");
                }
            }
        });
    }

    // Click anywhere on the row (not on a button) → open in Discord + mark read
    {
        let conn = conn.clone();
        let list_box = list_box.clone();
        let show_all = show_all.clone();
        let row_weak = row.downgrade();
        let read_state = read_state.clone();
        let gesture = GestureClick::new();
        gesture.connect_released(move |_, _, _, _| {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();

            if !read_state.get() {
                read_state.set(true);
                let _ = db::set_read(&*conn, n_id, true);
                if let Some(row) = row_weak.upgrade() {
                    row.remove_css_class("unread");
                    if !show_all.get() {
                        list_box.remove(&row);
                    }
                }
            }
        });
        hbox.add_controller(gesture);
    }

    row
}

fn discord_url(n: &Notification) -> String {
    match &n.guild_id {
        Some(gid) if !gid.is_empty() => {
            format!("https://discord.com/channels/{}/{}/{}", gid, n.channel_id, n.message_id)
        }
        _ => format!(
            "https://discord.com/channels/@me/{}/{}",
            n.channel_id, n.message_id
        ),
    }
}

fn format_timestamp(ts: Option<&str>) -> String {
    let Some(ts) = ts else {
        return String::new();
    };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) else {
        return String::new();
    };
    let secs = chrono::Utc::now()
        .signed_duration_since(dt.with_timezone(&chrono::Utc))
        .num_seconds();
    if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}
