mod queue;

use anyhow::{Context, Result, bail};
use ii::{
    cli::{RecvArgs, SendArgs},
    storage::{IiConfig, RelayProfile, S3Profile, WebDavProfile},
    transfer::TransferEvent,
};
use queue::{QueueDb, QueueTask, gui_data_dir, unix_time};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

slint::include_modules!();

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(0);
const RESIZE_BORDER_LOGICAL: f64 = 8.0;

struct AppState {
    config_path: PathBuf,
    config: IiConfig,
    queue: QueueDb,
}

#[derive(Debug)]
struct BackgroundEvent {
    task_id: String,
    event: TransferEvent,
}

#[derive(Debug, Clone)]
struct SendSelection {
    mode: String,
    profile: String,
    keep_sending: bool,
    delete_after_receive: bool,
    portable_webdav: bool,
}

fn main() -> Result<()> {
    ii::install_crypto_provider();

    let config_path = ii::storage::default_config_path()?;
    let config = ii::storage::load_config(&config_path)?;
    let data_dir = gui_data_dir();
    let queue = QueueDb::open(data_dir.join("queue.db"))?;
    let state = Rc::new(RefCell::new(AppState {
        config_path,
        config,
        queue,
    }));

    let window = MainWindow::new()?;
    window.set_gui_version(format!("GUI v{}", env!("CARGO_PKG_VERSION")).into());
    window.set_cli_version(format!("CLI v{}", ii::VERSION).into());
    refresh_all(&window, &state)?;
    if std::env::var_os("II_GUI_UI_REVIEW").is_some() {
        load_transfer_review_state(&window);
    }

    let (background_sender, background_receiver) = mpsc::channel::<BackgroundEvent>();
    install_navigation(&window);
    install_send_mode_selection(&window, Rc::clone(&state));
    install_window_controls(&window);
    install_file_dialogs(&window);
    install_profile_handlers(&window, Rc::clone(&state));
    install_config_directory_handler(&window, Rc::clone(&state));
    install_queue_handlers(&window, Rc::clone(&state));
    install_diagnostics(&window, Rc::clone(&state));
    install_transfer_handlers(&window, Rc::clone(&state), background_sender);
    install_event_pump(&window, Rc::clone(&state), background_receiver);

    window.show()?;
    install_titlebar_drag(&window);
    slint::run_event_loop()?;
    Ok(())
}

fn install_navigation(window: &MainWindow) {
    let weak = window.as_weak();
    window.on_set_page(move |page| {
        if let Some(window) = weak.upgrade() {
            window.set_page(page);
            window.set_status_text(page_status(page).into());
        }
    });
}

fn install_send_mode_selection(window: &MainWindow, state: Rc<RefCell<AppState>>) {
    let weak = window.as_weak();
    window.on_choose_send_mode(move |mode| {
        if let Some(window) = weak.upgrade() {
            let next_mode = mode.as_str();
            if next_mode != "local" && next_mode != "relay" {
                window.set_keep_sending(false);
            }
            if next_mode != "s3" && next_mode != "webdav" {
                window.set_delete_after_receive(false);
            }
            if next_mode != "webdav" {
                window.set_portable_webdav(false);
            }
            window.set_send_mode(mode);
            update_backend_profiles(&window, &state.borrow().config);
        }
    });
}

fn install_window_controls(window: &MainWindow) {
    let weak = window.as_weak();
    window.on_minimize_window(move || {
        if let Some(window) = weak.upgrade() {
            window.window().set_minimized(true);
        }
    });

    let weak = window.as_weak();
    window.on_maximize_window(move || {
        if let Some(window) = weak.upgrade() {
            let native = window.window();
            native.set_maximized(!native.is_maximized());
        }
    });

    let weak = window.as_weak();
    window.on_close_window(move || {
        if let Some(window) = weak.upgrade() {
            window.hide().ok();
        }
    });

    window.on_open_github(move || {
        let _ = open_url("https://github.com/zengyufei/ii");
    });
}

fn install_titlebar_drag(window: &MainWindow) {
    let last_pointer = Rc::new(RefCell::new(None::<winit::dpi::PhysicalPosition<f64>>));
    let pointer_for_events = Rc::clone(&last_pointer);

    window.window().on_winit_window_event(move |slint_window, event| {
        match event {
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                *pointer_for_events.borrow_mut() = Some(*position);
                let _ = slint_window.with_winit_window(|native| {
                    let direction = (!native.is_maximized())
                        .then(|| {
                            resize_direction_at(
                                *position,
                                native.inner_size(),
                                native.scale_factor(),
                            )
                        })
                        .flatten();
                    native.set_cursor(
                        direction
                            .map(winit::window::CursorIcon::from)
                            .unwrap_or(winit::window::CursorIcon::Default),
                    );
                });
            }
            winit::event::WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                let Some(pointer) = *pointer_for_events.borrow() else {
                    return EventResult::Propagate;
                };
                let started = slint_window
                    .with_winit_window(|native| {
                        if native.is_maximized() {
                            return false;
                        }
                        let scale_factor = native.scale_factor();
                        if let Some(direction) =
                            resize_direction_at(pointer, native.inner_size(), scale_factor)
                        {
                            return native.drag_resize_window(direction).is_ok();
                        }
                        let width = f64::from(native.inner_size().width);
                        if pointer.y <= 48.0 * scale_factor
                            && pointer.x >= 330.0 * scale_factor
                            && pointer.x < width - 430.0 * scale_factor
                        {
                            native.drag_window().is_ok()
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if started {
                    return EventResult::PreventDefault;
                }
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

fn resize_direction_at(
    pointer: winit::dpi::PhysicalPosition<f64>,
    size: winit::dpi::PhysicalSize<u32>,
    scale_factor: f64,
) -> Option<winit::window::ResizeDirection> {
    use winit::window::ResizeDirection;

    let border = RESIZE_BORDER_LOGICAL * scale_factor;
    let width = f64::from(size.width);
    let height = f64::from(size.height);
    let left = pointer.x <= border;
    let right = pointer.x >= width - border;
    let top = pointer.y <= border;
    let bottom = pointer.y >= height - border;

    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn install_file_dialogs(window: &MainWindow) {
    let weak = window.as_weak();
    window.on_choose_send_source(move || {
        let picked = rfd::FileDialog::new()
            .pick_file()
            .or_else(|| rfd::FileDialog::new().pick_folder());
        if let (Some(path), Some(window)) = (picked, weak.upgrade()) {
            window.set_send_path(path.display().to_string().into());
            window.set_send_source_meta(send_source_summary(&path).into());
        }
    });

    let weak = window.as_weak();
    window.on_choose_receive_directory(move || {
        if let (Some(path), Some(window)) = (rfd::FileDialog::new().pick_folder(), weak.upgrade()) {
            window.set_receive_directory(path.display().to_string().into());
        }
    });
}

fn send_source_summary(path: &Path) -> String {
    match path.metadata() {
        Ok(metadata) if metadata.is_file() => format!("{} · 已选择", format_file_size(metadata.len())),
        Ok(metadata) if metadata.is_dir() => "文件夹 · 已选择".into(),
        _ => "已选择".into(),
    }
}

fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1_000.0 && unit < UNITS.len() - 1 {
        value /= 1_000.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn install_profile_handlers(window: &MainWindow, state: Rc<RefCell<AppState>>) {
    let weak = window.as_weak();
    let state_for_new = Rc::clone(&state);
    window.on_new_profile(move |kind| {
        if let Some(window) = weak.upgrade() {
            reset_profile_editor(&window, kind.as_str());
            window.set_status_text(format!("新增 {kind} 配置。").into());
            let _ = refresh_profiles(&window, &state_for_new);
        }
    });

    let weak = window.as_weak();
    let state_for_select = Rc::clone(&state);
    window.on_select_profile(move |name| {
        if let Some(window) = weak.upgrade() {
            match populate_profile_editor(&window, &state_for_select.borrow().config, name.as_str())
            {
                Ok(()) => window.set_status_text(format!("正在编辑配置：{name}").into()),
                Err(err) => window.set_status_text(format!("读取配置失败：{err:#}").into()),
            }
        }
    });

    let weak = window.as_weak();
    let state_for_save = Rc::clone(&state);
    window.on_save_profile(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let result = save_profile_from_window(&window, &mut state_for_save.borrow_mut());
        match result {
            Ok(()) => {
                let _ = refresh_all(&window, &state_for_save);
                window.set_status_text("配置已保存。".into());
            }
            Err(err) => window.set_status_text(format!("保存配置失败：{err:#}").into()),
        }
    });

    let weak = window.as_weak();
    let state_for_delete = Rc::clone(&state);
    window.on_delete_profile(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let name = window.get_profile_name().to_string();
        let kind = window.get_profile_kind().to_string();
        if name.trim().is_empty() {
            window.set_status_text("请先选择配置。".into());
            return;
        }
        let result = {
            let mut state = state_for_delete.borrow_mut();
            match kind.as_str() {
                "S3" => {
                    state.config.storage.s3.remove(&name);
                }
                "WebDAV" => {
                    state.config.storage.webdav.remove(&name);
                }
                "TLS" => {
                    state.config.relay.remove(&name);
                }
                _ => {}
            }
            ii::storage::save_config(&state.config_path, &state.config)
        };
        match result {
            Ok(()) => {
                reset_profile_editor(&window, "S3");
                let _ = refresh_all(&window, &state_for_delete);
                window.set_status_text("配置已删除。".into());
            }
            Err(err) => window.set_status_text(format!("删除配置失败：{err:#}").into()),
        }
    });
}

fn install_config_directory_handler(window: &MainWindow, state: Rc<RefCell<AppState>>) {
    let weak = window.as_weak();
    let config_path = state.borrow().config_path.clone();
    window.on_open_config_directory(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let directory = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        match open_path(&directory) {
            Ok(()) => {
                window.set_status_text(format!("已打开配置目录：{}", directory.display()).into())
            }
            Err(err) => window.set_status_text(format!("打开配置目录失败：{err:#}").into()),
        }
    });
}

fn install_queue_handlers(window: &MainWindow, state: Rc<RefCell<AppState>>) {
    let weak = window.as_weak();
    let state_for_task = Rc::clone(&state);
    window.on_select_task(move |id| {
        if let Some(window) = weak.upgrade() {
            match state_for_task.borrow().queue.tasks() {
                Ok(tasks) => match tasks.into_iter().find(|task| task.id == id.as_str()) {
                    Some(task) => {
                        window.set_detail_name(task.name.into());
                        window.set_detail_text(format!("{} · {}", task.direction, task.status).into());
                        window.set_detail_ticket(
                            if task.ticket.is_empty() {
                                "传输码尚未创建".into()
                            } else {
                                task.ticket.into()
                            },
                        );
                        window.set_detail_method(task.method.into());
                        window.set_detail_progress(task.progress.into());
                        window.set_detail_time(format!("创建于 {}", task.created_at).into());
                        window.set_detail_destination(task.destination.into());
                        window.set_has_selected_task(true);
                    }
                    None => window.set_status_text("任务不存在。".into()),
                },
                Err(err) => window.set_status_text(format!("读取队列失败：{err:#}").into()),
            }
        }
    });

    let weak = window.as_weak();
    let state_for_clear = Rc::clone(&state);
    window.on_clear_completed(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        match state_for_clear.borrow().queue.clear_completed() {
            Ok(removed) => {
                let _ = refresh_tasks(&window, &state_for_clear);
                window.set_status_text(format!("已清理 {removed} 条完成记录。").into());
            }
            Err(err) => window.set_status_text(format!("清理失败：{err:#}").into()),
        }
    });

    let weak = window.as_weak();
    let state_for_compact = Rc::clone(&state);
    window.on_compact_database(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        match state_for_compact.borrow().queue.compact() {
            Ok(()) => {
                let _ = refresh_tasks(&window, &state_for_compact);
                window.set_status_text("数据库已整理。".into());
            }
            Err(err) => window.set_status_text(format!("整理失败：{err:#}").into()),
        }
    });

    let weak = window.as_weak();
    window.on_request_clear_history(move || {
        if let Some(window) = weak.upgrade() {
            window.set_confirm_clear_history(true);
        }
    });

    let weak = window.as_weak();
    window.on_clear_history(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        match state.borrow().queue.clear_all() {
            Ok(removed) => {
                let _ = refresh_tasks(&window, &state);
                window.set_detail_name("未选择任务".into());
                window.set_detail_text("从传输队列选择任务查看详情。".into());
                window.set_detail_ticket("".into());
                window.set_detail_method("".into());
                window.set_detail_progress("".into());
                window.set_detail_time("".into());
                window.set_detail_destination("".into());
                window.set_has_selected_task(false);
                window.set_confirm_clear_history(false);
                window.set_status_text(format!("已删除 {removed} 条传输历史。").into());
            }
            Err(err) => window.set_status_text(format!("删除失败：{err:#}").into()),
        }
    });
}

fn install_diagnostics(window: &MainWindow, state: Rc<RefCell<AppState>>) {
    let weak = window.as_weak();
    window.on_run_diagnostics(move || {
        if let Some(window) = weak.upgrade() {
            let state = state.borrow();
            let config_ok = state.config_path.exists();
            let queue_size = state.queue.file_size();
            window.set_diagnostics_text(
                format!(
                    "平台：{}\nCLI：v{}\n配置文件：{}\nSQLite 队列：{} B\nTLS 中继配置：{} 条\nS3 配置：{} 条\nWebDAV 配置：{} 条",
                    std::env::consts::OS,
                    ii::VERSION,
                    if config_ok { "可用" } else { "将首次保存时创建" },
                    queue_size,
                    state.config.relay.len(),
                    state.config.storage.s3.len(),
                    state.config.storage.webdav.len(),
                )
                .into(),
            );
            window.set_status_text("诊断完成。".into());
        }
    });
}

fn install_transfer_handlers(
    window: &MainWindow,
    state: Rc<RefCell<AppState>>,
    background_sender: mpsc::Sender<BackgroundEvent>,
) {
    let weak = window.as_weak();
    let state_for_send = Rc::clone(&state);
    let background_sender_for_send = background_sender.clone();
    window.on_start_send(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let path = PathBuf::from(window.get_send_path().to_string());
        let selection = SendSelection {
            mode: window.get_send_mode().to_string(),
            profile: window.get_selected_profile().to_string(),
            keep_sending: window.get_keep_sending(),
            delete_after_receive: window.get_delete_after_receive(),
            portable_webdav: window.get_portable_webdav(),
        };
        if window.get_send_path().trim().is_empty() || selection.mode.is_empty() {
            window.set_status_text("请选择发送内容和发送方式。".into());
            return;
        }
        let args = match build_send_args(
            &state_for_send.borrow().config,
            path.clone(),
            &selection,
        ) {
            Ok(args) => args,
            Err(err) => {
                window.set_status_text(format!("无法创建发送任务：{err:#}").into());
                return;
            }
        };
        let task_id = next_task_id("send");
        let task = QueueTask {
            id: task_id.clone(),
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("发送内容")
                .to_string(),
            direction: "发送".into(),
            method: mode_label(&selection.mode, &selection.profile),
            status: "准备中".into(),
            progress: "等待创建传输码".into(),
            ticket: String::new(),
            destination: "远端设备".into(),
            detail: "正在准备发送任务。".into(),
            created_at: unix_time(),
        };
        if let Err(err) = state_for_send.borrow().queue.upsert(&task) {
            window.set_status_text(format!("写入队列失败：{err:#}").into());
            return;
        }
        let _ = refresh_tasks(&window, &state_for_send);
        window.set_status_text("发送任务已加入队列。".into());
        let sender = background_sender_for_send.clone();
        thread::spawn(move || {
            let (event_sender, event_receiver) = mpsc::channel();
            let forward = sender.clone();
            let id = task_id.clone();
            thread::spawn(move || {
                while let Ok(event) = event_receiver.recv() {
                    let _ = forward.send(BackgroundEvent {
                        task_id: id.clone(),
                        event,
                    });
                }
            });
            let runtime = tokio::runtime::Runtime::new().expect("create transfer runtime");
            let _ = runtime.block_on(ii::transfer::send_with_events(args, event_sender));
        });
    });

    let weak = window.as_weak();
    let state_for_receive = state;
    window.on_start_receive(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let ticket = window.get_receive_ticket().to_string();
        let output = window.get_receive_directory().to_string();
        if ticket.trim().is_empty() || output.trim().is_empty() {
            window.set_status_text("请输入传输码并选择保存目录。".into());
            return;
        }
        let task_id = next_task_id("recv");
        let task = QueueTask {
            id: task_id.clone(),
            name: "接收内容".into(),
            direction: "接收".into(),
            method: "传输码".into(),
            status: "准备中".into(),
            progress: "等待连接".into(),
            ticket: ticket.clone(),
            destination: output.clone(),
            detail: "正在准备接收任务。".into(),
            created_at: unix_time(),
        };
        if let Err(err) = state_for_receive.borrow().queue.upsert(&task) {
            window.set_status_text(format!("写入队列失败：{err:#}").into());
            return;
        }
        let _ = refresh_tasks(&window, &state_for_receive);
        let args = RecvArgs {
            ticket,
            out_dir: Some(PathBuf::from(output)),
            stdout: false,
            overwrite: false,
            resume: false,
            local: false,
            trace: false,
        };
        let sender = background_sender.clone();
        thread::spawn(move || {
            let (event_sender, event_receiver) = mpsc::channel();
            let forward = sender.clone();
            let id = task_id.clone();
            thread::spawn(move || {
                while let Ok(event) = event_receiver.recv() {
                    let _ = forward.send(BackgroundEvent {
                        task_id: id.clone(),
                        event,
                    });
                }
            });
            let runtime = tokio::runtime::Runtime::new().expect("create transfer runtime");
            let _ = runtime.block_on(ii::transfer::recv_with_events(args, event_sender));
        });
    });
}

fn install_event_pump(
    window: &MainWindow,
    state: Rc<RefCell<AppState>>,
    receiver: mpsc::Receiver<BackgroundEvent>,
) {
    let weak = window.as_weak();
    let timer = Box::leak(Box::new(Timer::default()));
    timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
        while let Ok(message) = receiver.try_recv() {
            let Some(window) = weak.upgrade() else {
                return;
            };
            if let Err(err) = apply_background_event(&state, &message) {
                window.set_status_text(format!("更新队列失败：{err:#}").into());
                continue;
            }
            let _ = refresh_tasks(&window, &state);
            window.set_status_text(event_status(&message.event).into());
        }
    });
}

fn apply_background_event(state: &Rc<RefCell<AppState>>, message: &BackgroundEvent) -> Result<()> {
    let state = state.borrow();
    let mut task = state
        .queue
        .tasks()?
        .into_iter()
        .find(|task| task.id == message.task_id)
        .context("background task missing from queue")?;
    apply_transfer_event(&mut task, &message.event);
    state.queue.upsert(&task)
}

fn apply_transfer_event(task: &mut QueueTask, event: &TransferEvent) {
    match event {
        TransferEvent::Started => {
            task.status = "传输中".into();
            task.progress = "正在连接".into();
            task.detail = "传输任务已启动。".into();
        }
        TransferEvent::TicketReady(ticket) => {
            task.ticket = ticket.clone();
            task.status = "等待接收方".into();
            task.progress = "传输码已创建".into();
            task.detail = format!("传输码：{ticket}");
        }
        TransferEvent::Completed => {
            task.status = "已完成".into();
            task.progress = "100%".into();
            task.detail = "任务已完成。".into();
        }
        TransferEvent::Failed(error) => {
            task.status = "失败".into();
            task.progress = "未完成".into();
            task.detail = error.clone();
        }
    }
}

fn refresh_all(window: &MainWindow, state: &Rc<RefCell<AppState>>) -> Result<()> {
    refresh_tasks(window, state)?;
    refresh_profiles(window, state)?;
    let state = state.borrow();
    window.set_database_path(state.queue.path().display().to_string().into());
    window.set_database_summary(
        format!(
            "{} 个任务 · {} B",
            state.queue.tasks()?.len(),
            state.queue.file_size()
        )
        .into(),
    );
    window.set_diagnostics_text("点击“重新诊断”检查本机配置与 SQLite 队列。".into());
    Ok(())
}

fn refresh_tasks(window: &MainWindow, state: &Rc<RefCell<AppState>>) -> Result<()> {
    let tasks = state.borrow().queue.tasks()?;
    let rows = tasks
        .into_iter()
        .map(|task| TaskData {
            id: task.id.into(),
            name: task.name.into(),
            direction: task.direction.into(),
            method: task.method.into(),
            status: task.status.into(),
            progress: task.progress.into(),
            ticket: task.ticket.into(),
            destination: task.destination.into(),
            detail: task.detail.into(),
        })
        .collect::<Vec<_>>();
    window.set_tasks(ModelRc::new(VecModel::from(rows)));
    let state = state.borrow();
    window.set_database_summary(
        format!(
            "{} 个任务 · {} B",
            window.get_tasks().row_count(),
            state.queue.file_size()
        )
        .into(),
    );
    Ok(())
}

// This is an in-memory visual review state for screenshot comparison only.
fn load_transfer_review_state(window: &MainWindow) {
    let tasks = vec![
        TaskData {
            id: "review-backup".into(),
            name: "project-backup.zip".into(),
            direction: "发送".into(),
            method: "指定中继".into(),
            status: "传输中".into(),
            progress: "62% · 18.4 MB/s".into(),
            ticket: "ii1k7v...demo-transfer-code".into(),
            destination: "1.4 GB · 今天 12:37".into(),
            detail: "发送中 · 1.4 GB".into(),
        },
        TaskData {
            id: "review-photos".into(),
            name: "photos/".into(),
            direction: "接收".into(),
            method: "WebDAV".into(),
            status: "已完成".into(),
            progress: "已保存到下载".into(),
            ticket: "ii1k7v...received-webdav-code".into(),
            destination: "824 MB · 今天 12:41".into(),
            detail: "已接收 · 824 MB".into(),
        },
        TaskData {
            id: "review-archive".into(),
            name: "archive.tar.gz".into(),
            direction: "发送".into(),
            method: "局域网".into(),
            status: "等待中".into(),
            progress: "传输码已复制".into(),
            ticket: "ii1k7v...local-transfer-code".into(),
            destination: "4.8 GB · 等待接收方".into(),
            detail: "等待中 · 4.8 GB".into(),
        },
    ];
    window.set_tasks(ModelRc::new(VecModel::from(tasks)));
    window.set_send_path("project-backup.zip".into());
    window.set_send_source_meta("1.4 GB · 已选择".into());
    window.set_receive_ticket("ii1k7v...demo-transfer-code".into());
    window.set_receive_directory("~/下载".into());
    window.set_detail_name("project-backup.zip".into());
    window.set_detail_text("发送中 · 1.4 GB".into());
    window.set_detail_ticket("ii1k7v...demo-transfer-code".into());
    window.set_detail_method("指定中继 · 公司中继".into());
    window.set_detail_progress("62% · 18.4 MB/s".into());
    window.set_detail_time("今天 12:37".into());
    window.set_detail_destination("远端设备".into());
    window.set_has_selected_task(true);
}

fn refresh_profiles(window: &MainWindow, state: &Rc<RefCell<AppState>>) -> Result<()> {
    let config = &state.borrow().config;
    let mut rows = Vec::new();
    rows.extend(config.storage.s3.iter().map(|(name, profile)| ProfileData {
        name: name.clone().into(),
        kind: "S3".into(),
        description: profile.bucket.clone().into(),
    }));
    rows.extend(
        config
            .storage
            .webdav
            .iter()
            .map(|(name, profile)| ProfileData {
                name: name.clone().into(),
                kind: "WebDAV".into(),
                description: profile.url.clone().into(),
            }),
    );
    rows.extend(config.relay.iter().map(|(name, profile)| ProfileData {
        name: name.clone().into(),
        kind: "TLS".into(),
        description: profile.url.clone().into(),
    }));
    window.set_profiles(ModelRc::new(VecModel::from(rows)));
    update_backend_profiles(window, config);
    Ok(())
}

fn update_backend_profiles(window: &MainWindow, config: &IiConfig) {
    let profiles = profile_names_for_mode(config, window.get_send_mode().as_str());
    window.set_backend_profiles(ModelRc::new(VecModel::from(
        profiles
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    )));
    window.set_selected_profile("".into());
}

fn profile_names_for_mode(config: &IiConfig, mode: &str) -> Vec<String> {
    match mode {
        "s3" => config.storage.s3.keys().cloned().collect(),
        "webdav" => config.storage.webdav.keys().cloned().collect(),
        "relay" => config.relay.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

fn reset_profile_editor(window: &MainWindow, kind: &str) {
    window.set_profile_kind(kind.into());
    window.set_profile_name("".into());
    window.set_profile_endpoint("".into());
    window.set_profile_bucket("".into());
    window.set_profile_user("".into());
    window.set_profile_secret("".into());
    window.set_profile_provider("generic-s3".into());
    window.set_profile_prefix("ii/".into());
    window.set_profile_region("auto".into());
    window.set_profile_presign_ttl("86400".into());
    window.set_profile_webdav_auth("Basic".into());
    window.set_profile_self_signed(false);
}

fn populate_profile_editor(window: &MainWindow, config: &IiConfig, name: &str) -> Result<()> {
    if let Some(profile) = config.storage.s3.get(name) {
        window.set_profile_kind("S3".into());
        window.set_profile_name(name.into());
        window.set_profile_endpoint(profile.endpoint.clone().into());
        window.set_profile_bucket(profile.bucket.clone().into());
        window.set_profile_user(profile.access_key_id.clone().into());
        window.set_profile_secret(profile.secret_access_key.clone().into());
        window.set_profile_provider(profile.provider.clone().into());
        window.set_profile_prefix(profile.prefix.clone().into());
        window.set_profile_region(profile.region.clone().into());
        window.set_profile_presign_ttl(profile.presign_ttl_seconds.to_string().into());
        window.set_profile_webdav_auth("Basic".into());
        window.set_profile_self_signed(false);
        return Ok(());
    }
    if let Some(profile) = config.storage.webdav.get(name) {
        window.set_profile_kind("WebDAV".into());
        window.set_profile_name(name.into());
        window.set_profile_endpoint(profile.url.clone().into());
        window.set_profile_bucket("".into());
        window.set_profile_user(profile.username.clone().into());
        window.set_profile_secret(profile.password.clone().into());
        window.set_profile_provider("generic-s3".into());
        window.set_profile_prefix(profile.remote_dir.clone().into());
        window.set_profile_region("auto".into());
        window.set_profile_presign_ttl("86400".into());
        window.set_profile_webdav_auth(
            match profile.auth {
                ii::storage::WebDavAuth::Basic => "Basic",
                ii::storage::WebDavAuth::Digest => "Digest",
            }
            .into(),
        );
        window.set_profile_self_signed(false);
        return Ok(());
    }
    if let Some(profile) = config.relay.get(name) {
        window.set_profile_kind("TLS".into());
        window.set_profile_name(name.into());
        window.set_profile_endpoint(profile.url.clone().into());
        window.set_profile_bucket("".into());
        window.set_profile_user("".into());
        window.set_profile_secret("".into());
        window.set_profile_provider("generic-s3".into());
        window.set_profile_prefix("ii/".into());
        window.set_profile_region("auto".into());
        window.set_profile_presign_ttl("86400".into());
        window.set_profile_webdav_auth("Basic".into());
        window.set_profile_self_signed(profile.accept_self_signed);
        return Ok(());
    }
    bail!("unknown profile {name}")
}

fn save_profile_from_window(window: &MainWindow, state: &mut AppState) -> Result<()> {
    let name = window.get_profile_name().trim().to_string();
    if name.is_empty() {
        bail!("配置名称不能为空");
    }
    match window.get_profile_kind().as_str() {
        "S3" => {
            let presign_ttl_seconds = window
                .get_profile_presign_ttl()
                .trim()
                .parse()
                .context("下载链接有效期必须是秒数")?;
            let profile = S3Profile {
                provider: window.get_profile_provider().to_string(),
                account_id: None,
                bucket: window.get_profile_bucket().to_string(),
                endpoint: window.get_profile_endpoint().to_string(),
                region: window.get_profile_region().to_string(),
                access_key_id: window.get_profile_user().to_string(),
                secret_access_key: window.get_profile_secret().to_string(),
                prefix: window.get_profile_prefix().to_string(),
                presign_ttl_seconds,
                path_style: true,
            };
            ii::storage::validate_s3_profile(&profile)?;
            state.config.storage.s3.insert(name, profile);
        }
        "WebDAV" => {
            let profile = WebDavProfile {
                url: window.get_profile_endpoint().to_string(),
                username: window.get_profile_user().to_string(),
                password: window.get_profile_secret().to_string(),
                remote_dir: window.get_profile_prefix().to_string(),
                auth: match window.get_profile_webdav_auth().as_str() {
                    "Digest" => ii::storage::WebDavAuth::Digest,
                    _ => ii::storage::WebDavAuth::Basic,
                },
            };
            ii::storage::validate_webdav_profile(&profile)?;
            state.config.storage.webdav.insert(name, profile);
        }
        "TLS" => {
            let profile = RelayProfile {
                url: window.get_profile_endpoint().to_string(),
                accept_self_signed: window.get_profile_self_signed(),
            };
            profile.validate()?;
            state.config.relay.insert(name, profile);
        }
        kind => bail!("未知配置类型 {kind}"),
    }
    ii::storage::save_config(&state.config_path, &state.config)
}

fn build_send_args(
    config: &IiConfig,
    path: PathBuf,
    selection: &SendSelection,
) -> Result<SendArgs> {
    let mut args = SendArgs {
        path: Some(path),
        ..Default::default()
    };
    match selection.mode.as_str() {
        "local" => {
            args.local = true;
            args.keep_alive = selection.keep_sending;
        }
        "s3" => {
            ensure_profile(&config.storage.s3, &selection.profile, "S3")?;
            args.s3 = true;
            args.profile = Some(selection.profile.clone());
            args.delete_after_recv = selection.delete_after_receive;
        }
        "webdav" => {
            ensure_profile(&config.storage.webdav, &selection.profile, "WebDAV")?;
            args.webdav = true;
            args.profile = Some(selection.profile.clone());
            args.delete_after_recv = selection.delete_after_receive;
            args.portable_webdav = selection.portable_webdav;
        }
        "relay" => {
            let relay = config
                .relay
                .get(&selection.profile)
                .context("请选择 TLS 中继配置")?;
            relay.validate()?;
            args.relay = Some(relay.url.parse().context("parse relay profile URL")?);
            args.accept_self_signed_relay = relay.accept_self_signed;
            args.keep_alive = selection.keep_sending;
        }
        _ => bail!("请选择发送方式"),
    }
    Ok(args)
}

fn ensure_profile<T>(profiles: &BTreeMap<String, T>, name: &str, kind: &str) -> Result<()> {
    if name.trim().is_empty() || !profiles.contains_key(name) {
        bail!("请选择 {kind} 配置");
    }
    Ok(())
}

fn mode_label(mode: &str, profile: &str) -> String {
    match mode {
        "local" => "仅局域网".into(),
        "relay" => format!("指定中继 · {profile}"),
        "s3" => format!("S3 · {profile}"),
        "webdav" => format!("WebDAV · {profile}"),
        _ => "未选择".into(),
    }
}

fn page_status(page: i32) -> &'static str {
    match page {
        0 => "传输页：创建发送或接收任务。",
        1 => "存储页：管理 S3、WebDAV 与 TLS 配置。",
        2 => "诊断页：检查本机状态。",
        3 => "设置页：管理 SQLite 队列数据库。",
        _ => "就绪",
    }
}

fn event_status(event: &TransferEvent) -> &'static str {
    match event {
        TransferEvent::Started => "传输已开始。",
        TransferEvent::TicketReady(_) => "传输码已创建。",
        TransferEvent::Completed => "传输已完成。",
        TransferEvent::Failed(_) => "传输失败。",
    }
}

fn next_task_id(direction: &str) -> String {
    let sequence = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("{direction}-{:x}-{:x}", unix_time(), sequence)
}

fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}

fn open_path(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer").arg(path).spawn()?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(path).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    std::process::Command::new("xdg-open").arg(path).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> QueueTask {
        QueueTask {
            id: "task-1".into(),
            name: "example.txt".into(),
            direction: "发送".into(),
            method: "仅局域网".into(),
            status: "准备中".into(),
            progress: "等待创建传输码".into(),
            ticket: String::new(),
            destination: "远端设备".into(),
            detail: String::new(),
            created_at: 1,
        }
    }

    #[test]
    fn transfer_events_follow_queue_state_machine() {
        let mut task = task();
        apply_transfer_event(&mut task, &TransferEvent::Started);
        assert_eq!(task.status, "传输中");

        apply_transfer_event(&mut task, &TransferEvent::TicketReady("ii1example".into()));
        assert_eq!(task.status, "等待接收方");
        assert_eq!(task.ticket, "ii1example");

        apply_transfer_event(&mut task, &TransferEvent::Completed);
        assert_eq!(task.status, "已完成");
        assert_eq!(task.progress, "100%");

        apply_transfer_event(&mut task, &TransferEvent::Failed("network error".into()));
        assert_eq!(task.status, "失败");
        assert_eq!(task.detail, "network error");
    }

    fn config_with_send_profiles() -> IiConfig {
        let mut config = IiConfig::default();
        config
            .storage
            .s3
            .insert("对象存储".into(), S3Profile::empty_cloudflare());
        config
            .storage
            .webdav
            .insert("团队 WebDAV".into(), WebDavProfile::empty());
        config.relay.insert(
            "实验中继".into(),
            RelayProfile {
                url: "https://relay.example.com".into(),
                accept_self_signed: true,
            },
        );
        config
    }

    fn send_selection(mode: &str, profile: &str) -> SendSelection {
        SendSelection {
            mode: mode.into(),
            profile: profile.into(),
            keep_sending: false,
            delete_after_receive: false,
            portable_webdav: false,
        }
    }

    #[test]
    fn send_mode_only_exposes_matching_profiles() {
        let config = config_with_send_profiles();

        assert_eq!(profile_names_for_mode(&config, "s3"), ["对象存储"]);
        assert_eq!(profile_names_for_mode(&config, "webdav"), ["团队 WebDAV"]);
        assert_eq!(profile_names_for_mode(&config, "relay"), ["实验中继"]);
        assert!(profile_names_for_mode(&config, "local").is_empty());
    }

    #[test]
    fn local_send_maps_keep_sending_only() {
        let config = config_with_send_profiles();
        let mut selection = send_selection("local", "");
        selection.keep_sending = true;

        let args = build_send_args(&config, PathBuf::from("example.txt"), &selection).unwrap();
        assert!(args.local);
        assert!(args.keep_alive);
        assert!(!args.s3);
        assert!(!args.webdav);
        assert!(!args.delete_after_recv);
        assert!(!args.portable_webdav);
    }

    #[test]
    fn relay_send_maps_keep_sending_and_profile() {
        let config = config_with_send_profiles();
        let mut selection = send_selection("relay", "实验中继");
        selection.keep_sending = true;

        let args = build_send_args(&config, PathBuf::from("example.txt"), &selection).unwrap();
        assert!(args.relay.is_some());
        assert!(args.accept_self_signed_relay);
        assert!(args.keep_alive);
        assert!(!args.delete_after_recv);
        assert!(!args.portable_webdav);
    }

    #[test]
    fn s3_send_maps_delete_after_receive_only() {
        let config = config_with_send_profiles();
        let mut selection = send_selection("s3", "对象存储");
        selection.delete_after_receive = true;
        selection.keep_sending = true;

        let args = build_send_args(&config, PathBuf::from("example.txt"), &selection).unwrap();
        assert!(args.s3);
        assert_eq!(args.profile.as_deref(), Some("对象存储"));
        assert!(args.delete_after_recv);
        assert!(!args.keep_alive);
        assert!(!args.portable_webdav);
    }

    #[test]
    fn webdav_send_maps_delete_and_portable_credentials_only() {
        let config = config_with_send_profiles();
        let mut selection = send_selection("webdav", "团队 WebDAV");
        selection.delete_after_receive = true;
        selection.portable_webdav = true;
        selection.keep_sending = true;

        let args = build_send_args(&config, PathBuf::from("example.txt"), &selection).unwrap();
        assert!(args.webdav);
        assert_eq!(args.profile.as_deref(), Some("团队 WebDAV"));
        assert!(args.delete_after_recv);
        assert!(args.portable_webdav);
        assert!(!args.keep_alive);
    }

    #[test]
    fn source_size_labels_are_compact_and_consistent() {
        assert_eq!(format_file_size(999), "999 B");
        assert_eq!(format_file_size(1_400), "1.4 KB");
        assert_eq!(format_file_size(1_400_000), "1.4 MB");
        assert_eq!(format_file_size(1_400_000_000), "1.4 GB");
    }

    #[test]
    fn title_bar_buttons_share_their_glyph_and_hit_geometry() {
        let ui = include_str!("../ui/main.slint");
        let definition = ui
            .split("component TitleBarButton inherits Rectangle {")
            .nth(1)
            .and_then(|rest| rest.split("component ListButton inherits Rectangle {").next())
            .expect("title bar control component");

        assert!(definition.lines().any(|line| line.trim() == "Text {"));
        assert!(definition.lines().any(|line| line.trim() == "touch := TouchArea {"));
        assert!(definition.matches("width: parent.width;").count() >= 2);
        assert!(definition.contains("Rectangle { x: 0px; y: 0px; width: 1px;"));
        assert!(!definition.contains("x: parent.width"));
        assert!(!ui.contains("minimize-touch := TouchArea"));
        assert!(!ui.contains("maximize-touch := TouchArea"));
        assert!(!ui.contains("window-close-touch := TouchArea"));
        assert!(ui.contains("titlebar-controls := HorizontalLayout {"));
        assert!(!ui.contains("TitleBarButton { x: parent.width -"));
    }

    #[test]
    fn compact_send_form_uses_consistent_label_columns() {
        let ui = include_str!("../ui/main.slint");
        let label = ui
            .split("component SendFormLabel inherits Text {")
            .nth(1)
            .and_then(|rest| rest.split("component SendProfileField inherits Rectangle {").next())
            .expect("shared compact send form label");

        assert!(label.contains("width: 70px;"));
        assert!(label.contains("vertical-stretch: 1;"));
        assert!(label.contains("vertical-alignment: center;"));
        assert!(ui.contains("component SendProfileField inherits Rectangle {"));
        assert!(ui.contains("height: root.send-mode == \"webdav\" ? 14px : 0px;"));
    }

    #[test]
    fn title_bar_drag_region_uses_the_native_window_drag_operation() {
        let ui = include_str!("../ui/main.slint");
        let drag_region = ui
            .split("titlebar-drag-area := TouchArea {")
            .nth(1)
            .and_then(|rest| rest.split("titlebar-controls := HorizontalLayout {").next())
            .expect("dedicated title bar drag area before window controls");

        assert!(drag_region.contains("x: 330px;"));
        assert!(drag_region.contains("width: parent.width - 760px;"));
        assert!(drag_region.contains("height: parent.height;"));

        let source = include_str!("main.rs");
        assert!(source.contains("window.window().on_winit_window_event"));
        assert!(source.contains("MouseButton::Left"));
        assert!(source.contains("pointer.x >= 330.0 * scale_factor"));
        assert!(source.contains("pointer.x < width - 430.0 * scale_factor"));
        assert!(source.contains("with_winit_window(|native| native.drag_window())"));
    }

    #[test]
    fn resize_hit_test_covers_each_edge_and_corner() {
        let size = winit::dpi::PhysicalSize::new(1_000, 800);

        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(4.0, 400.0), size, 1.0),
            Some(winit::window::ResizeDirection::West)
        );
        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(500.0, 4.0), size, 1.0),
            Some(winit::window::ResizeDirection::North)
        );
        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(996.0, 4.0), size, 1.0),
            Some(winit::window::ResizeDirection::NorthEast)
        );
        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(996.0, 400.0), size, 1.0),
            Some(winit::window::ResizeDirection::East)
        );
        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(996.0, 796.0), size, 1.0),
            Some(winit::window::ResizeDirection::SouthEast)
        );
        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(500.0, 796.0), size, 1.0),
            Some(winit::window::ResizeDirection::South)
        );
        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(4.0, 796.0), size, 1.0),
            Some(winit::window::ResizeDirection::SouthWest)
        );
        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(4.0, 4.0), size, 1.0),
            Some(winit::window::ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction_at(winit::dpi::PhysicalPosition::new(500.0, 400.0), size, 1.0),
            None
        );
    }
}
