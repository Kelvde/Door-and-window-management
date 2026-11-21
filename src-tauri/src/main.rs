// src-tauri/src/main.rs
// 门窗管理软件 - 纯 Rust 单一 exe 版(方案 B)
// 不依赖 Node、不依赖浏览器、数据全在软件目录(D 盘)。
// 前端通过 Tauri command 调用 Rust 后端,不再走 HTTP/localhost。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;
mod db;
mod export;
mod im;
mod media;
mod model;
mod models_seed;
mod order;
mod plugin;
mod print;
mod shell;
mod single;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 已由用户确认退出：此时不再拦截 CloseRequested，避免窗口被永久钉死。
static ALLOW_EXIT: AtomicBool = AtomicBool::new(false);
/// 前端关闭守卫是否注册成功：未注册时后端不拦截关闭，保证「至少能关掉」。
static CLOSE_GUARD_READY: AtomicBool = AtomicBool::new(false);

use export::{ExportResult, ExportService};
use model::{
    Accessory, BackupRecord, CheckDetail, CheckRow, Component, Contract, ContractInput, Customer,
    CustomerInput, DraftRecord, Glass, GlassType, Meta, Order, OrderInput, PluginInfo,
    ProductPreset, ProductionStep, Profile, ProfileLib, ScrewRule,
};
use plugin::PluginHost;
use print::{doc_type_name, PrintService, DOC_TYPES};
use rusqlite::Connection;
use tauri::{Emitter, Manager};

/// 全局内核状态,通过 Tauri 管理
struct AppState {
    order_service: order::OrderService,
    backup_service: backup::BackupService,
    print_service: PrintService,
    export_service: ExportService,
    plugin_host: Arc<PluginHost>,
}

// ---------- Tauri Commands ----------

#[tauri::command]
fn meta(state: tauri::State<'_, AppState>) -> Result<Meta, String> {
    let data_dir = db::data_dir().to_string_lossy().to_string();
    let company_name = {
        let conn = state.order_service.db.lock().unwrap();
        db::get_config(&conn, "company.name").unwrap_or_default()
    };
    let operator_name = {
        let conn = state.order_service.db.lock().unwrap();
        db::get_config(&conn, "operator.name").unwrap_or_else(|| "管理员".to_string())
    };
    Ok(Meta {
        name: "门窗管理软件".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        backup_types: backup::BACKUP_TYPES.iter().map(|s| s.to_string()).collect(),
        doc_types: DOC_TYPES.iter().map(|s| s.to_string()).collect(),
        data_dir,
        platform: std::env::consts::OS.to_string(),
        company_name,
        operator_name,
    })
}

#[tauri::command]
fn set_config(state: tauri::State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    let conn = state.order_service.db.lock().unwrap();
    db::set_config(&conn, &key, &value)
}

#[tauri::command]
fn get_config(state: tauri::State<'_, AppState>, key: String) -> Result<Option<String>, String> {
    let conn = state.order_service.db.lock().unwrap();
    Ok(db::get_config(&conn, &key))
}

/// 列出全部配置项。设置页保存前后做全表对比，防止「改了公司名其它资料没了」。
#[tauri::command]
fn config_list(state: tauri::State<'_, AppState>) -> Result<Vec<(String, String)>, String> {
    let conn = state.order_service.db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT key, value FROM app_config ORDER BY key")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

// ---- 订单 ----
#[tauri::command]
fn order_list(state: tauri::State<'_, AppState>) -> Result<Vec<Order>, String> {
    state.order_service.list()
}

#[tauri::command]
fn order_get(state: tauri::State<'_, AppState>, id: i64) -> Result<Order, String> {
    state.order_service.get(id)
}

#[tauri::command]
fn order_create(state: tauri::State<'_, AppState>, input: OrderInput) -> Result<Order, String> {
    state.order_service.create(input)
}

#[tauri::command]
fn order_update(
    state: tauri::State<'_, AppState>,
    id: i64,
    input: OrderInput,
) -> Result<Order, String> {
    state.order_service.update(id, input)
}

/// 编辑订单时先清空明细，再按新内容重写
#[tauri::command]
fn order_clear_details(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.clear_details(id)
}

/// 软件内预览：把勾选订单按单据样板渲染成一段 HTML 返回给前端（不落盘、不走打印门禁）
#[tauri::command]
fn render_preview(
    state: tauri::State<'_, AppState>,
    order_ids: Vec<i64>,
    doc_types: Vec<String>,
) -> Result<String, String> {
    if order_ids.is_empty() {
        return Err("请先勾选订单".to_string());
    }
    state.print_service.share_html(&order_ids, &doc_types)
}

#[tauri::command]
fn order_set_status(
    state: tauri::State<'_, AppState>,
    id: i64,
    status: String,
) -> Result<Order, String> {
    state.order_service.set_status(id, &status)
}

#[tauri::command]
fn order_remove(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.remove(id)
}

// ---- 客户 ----
#[tauri::command]
fn customer_list(state: tauri::State<'_, AppState>) -> Result<Vec<Customer>, String> {
    state.order_service.customer_list()
}

#[tauri::command]
fn customer_save(state: tauri::State<'_, AppState>, input: CustomerInput) -> Result<Customer, String> {
    state.order_service.customer_save(input)
}

#[tauri::command]
fn customer_remove(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.customer_remove(id)
}

// ---- 合同 ----
#[tauri::command]
fn contract_list(state: tauri::State<'_, AppState>) -> Result<Vec<Contract>, String> {
    state.order_service.contract_list()
}

#[tauri::command]
fn contract_save(state: tauri::State<'_, AppState>, input: ContractInput) -> Result<Contract, String> {
    state.order_service.contract_save(input)
}

#[tauri::command]
fn contract_remove(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.contract_remove(id)
}

// ---- 构件 ----
#[tauri::command]
fn component_list(state: tauri::State<'_, AppState>, order_id: i64) -> Result<Vec<Component>, String> {
    state.order_service.list_components(order_id)
}

#[tauri::command]
fn component_add(state: tauri::State<'_, AppState>, comp: Component) -> Result<Component, String> {
    state.order_service.add_component(comp)
}

#[tauri::command]
fn component_remove(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.remove_component(id)
}

#[tauri::command]
fn component_descendants(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<Vec<Component>, String> {
    state.order_service.descendants(id)
}

// ---- 型材 ----
#[tauri::command]
fn profile_list(state: tauri::State<'_, AppState>, order_id: i64) -> Result<Vec<Profile>, String> {
    state.order_service.profile_list(order_id)
}

#[tauri::command]
fn profile_save(state: tauri::State<'_, AppState>, p: Profile) -> Result<Profile, String> {
    state.order_service.profile_save(p)
}

#[tauri::command]
fn profile_remove(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.profile_remove(id)
}

// ---- 玻璃 ----
#[tauri::command]
fn glass_list(state: tauri::State<'_, AppState>, order_id: i64) -> Result<Vec<Glass>, String> {
    state.order_service.glass_list(order_id)
}

#[tauri::command]
fn glass_save(state: tauri::State<'_, AppState>, g: Glass) -> Result<Glass, String> {
    state.order_service.glass_save(g)
}

#[tauri::command]
fn glass_remove(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.glass_remove(id)
}

// ---- 配件 ----
#[tauri::command]
fn accessory_list(state: tauri::State<'_, AppState>, order_id: i64) -> Result<Vec<Accessory>, String> {
    state.order_service.accessory_list(order_id)
}

#[tauri::command]
fn accessory_save(state: tauri::State<'_, AppState>, a: Accessory) -> Result<Accessory, String> {
    state.order_service.accessory_save(a)
}

#[tauri::command]
fn accessory_remove(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.accessory_remove(id)
}

// ---- 生产环节 ----
#[tauri::command]
fn step_list(state: tauri::State<'_, AppState>, order_id: i64) -> Result<Vec<ProductionStep>, String> {
    state.order_service.step_list(order_id)
}

#[tauri::command]
fn step_toggle(
    state: tauri::State<'_, AppState>,
    order_id: i64,
    step_name: String,
    done: bool,
) -> Result<(), String> {
    state.order_service.step_toggle(order_id, &step_name, done)
}

// ---- 产品预设 ----
#[tauri::command]
fn preset_list(state: tauri::State<'_, AppState>) -> Result<Vec<ProductPreset>, String> {
    state.order_service.preset_list()
}

#[tauri::command]
fn preset_save(state: tauri::State<'_, AppState>, p: ProductPreset) -> Result<ProductPreset, String> {
    state.order_service.preset_save(p)
}

#[tauri::command]
fn preset_remove(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    state.order_service.preset_remove(id)
}

// ---- 玻璃类型清单 ----
#[tauri::command]
fn glass_type_list(state: tauri::State<'_, AppState>) -> Result<Vec<GlassType>, String> {
    state.order_service.glass_type_list()
}

// ---- 产品类型→型材库 ----
#[tauri::command]
fn profile_lib_list(
    state: tauri::State<'_, AppState>,
    product_type: String,
    series: String,
) -> Result<Vec<ProfileLib>, String> {
    state.order_service.profile_lib_list(&product_type, &series)
}

#[tauri::command]
fn profile_lib_types(state: tauri::State<'_, AppState>) -> Result<Vec<(String, String)>, String> {
    state.order_service.profile_lib_types()
}

/// 量表：总量表（customer 为空）或指定客户的账
#[tauri::command]
fn stats_overview(
    state: tauri::State<'_, AppState>,
    customer: Option<String>,
) -> Result<order::StatsResult, String> {
    state.order_service.stats(customer.as_deref())
}

/// 量表：客户清单（客户名 / 订单数 / 金额 / 面积）
#[tauri::command]
fn stats_customers(state: tauri::State<'_, AppState>) -> Result<Vec<order::CustomerStat>, String> {
    state.order_service.stats_customers()
}

/// 全部型材名称（异形订单手工添加型材时的候选）
#[tauri::command]
fn profile_lib_names(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    state.order_service.profile_lib_names()
}

#[tauri::command]
fn profile_lib_save(state: tauri::State<'_, AppState>, p: ProfileLib) -> Result<ProfileLib, String> {
    state.order_service.profile_lib_save(p)
}

#[tauri::command]
fn calc_profile_sizes(
    state: tauri::State<'_, AppState>,
    product_type: String,
    series: String,
    width: f64,
    height: f64,
    fan_count: i64,
    window_count: Option<i64>,
    door_height: Option<f64>,
    wall_thickness: Option<f64>,
    track_type: Option<String>,
    overlap_count: Option<i64>,
    double_glass: Option<bool>,
) -> Result<Vec<(String, f64, i64, String)>, String> {
    let window_count = window_count.unwrap_or(0);
    let door_height = door_height.unwrap_or(0.0);
    let wall_thickness = wall_thickness.unwrap_or(0.0);
    let track_type = track_type.unwrap_or_else(|| "地轨".to_string());
    let overlap_count = overlap_count.unwrap_or(0);
    let double_glass = double_glass.unwrap_or(false);
    state.order_service.calc_profile_sizes(
        &product_type, &series, width, height, fan_count,
        window_count, door_height, wall_thickness, &track_type, overlap_count, double_glass,
    )
}

#[tauri::command]
fn calc_glass_sizes(
    state: tauri::State<'_, AppState>,
    product_type: String,
    series: String,
    width: f64,
    height: f64,
    fan_count: i64,
    window_count: Option<i64>,
    door_height: Option<f64>,
    wall_thickness: Option<f64>,
    track_type: Option<String>,
    overlap_count: Option<i64>,
    double_glass: Option<bool>,
) -> Result<Vec<(String, f64, f64, i64)>, String> {
    let window_count = window_count.unwrap_or(0);
    let door_height = door_height.unwrap_or(0.0);
    let wall_thickness = wall_thickness.unwrap_or(0.0);
    let track_type = track_type.unwrap_or_else(|| "地轨".to_string());
    let overlap_count = overlap_count.unwrap_or(0);
    let double_glass = double_glass.unwrap_or(false);
    state.order_service.calc_glass_sizes(
        &product_type, &series, width, height, fan_count,
        window_count, door_height, wall_thickness, &track_type, overlap_count, double_glass,
    )
}

// ---- 插件 ----
#[tauri::command]
fn plugin_list(state: tauri::State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    Ok(state.plugin_host.list_plugins())
}

#[tauri::command]
fn plugin_enable(
    state: tauri::State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state.plugin_host.set_enabled(&id, enabled)
}

#[tauri::command]
fn plugin_uninstall(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.plugin_host.uninstall(&id)
}

#[tauri::command]
fn plugin_install_demo(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.plugin_host.install_demo()
}

// ---- 备份 ----
#[tauri::command]
fn backup_list(
    state: tauri::State<'_, AppState>,
    ty: String,
) -> Result<Vec<BackupRecord>, String> {
    state.backup_service.list(&ty)
}

#[tauri::command]
fn backup_run(state: tauri::State<'_, AppState>, ty: String) -> Result<BackupRecord, String> {
    state.backup_service.run(&ty)
}

#[tauri::command]
fn backup_delete(
    state: tauri::State<'_, AppState>,
    ty: String,
    name: String,
) -> Result<(), String> {
    state.backup_service.delete(&ty, &name)
}

#[tauri::command]
fn backup_clear(state: tauri::State<'_, AppState>, ty: String) -> Result<(), String> {
    state.backup_service.clear(&ty)
}

// ---- 打印 ----
#[tauri::command]
fn print_check_detail(
    state: tauri::State<'_, AppState>,
    order_id: i64,
) -> Result<CheckDetail, String> {
    state.print_service.build_check_detail(order_id)
}

#[tauri::command]
fn print_check_page(
    state: tauri::State<'_, AppState>,
    order_id: i64,
) -> Result<Vec<CheckRow>, String> {
    state.print_service.build_check_page(order_id)
}

#[tauri::command]
fn print_confirm(
    state: tauri::State<'_, AppState>,
    order_id: i64,
    operator: Option<String>,
) -> Result<(), String> {
    state
        .print_service
        .confirm_size(order_id, operator.as_deref().unwrap_or("admin"))
}

#[tauri::command]
fn print_run(
    state: tauri::State<'_, AppState>,
    order_id: i64,
    doc_type: String,
    output: String,
) -> Result<String, String> {
    state.print_service.print(order_id, &doc_type, &output)
}

#[tauri::command]
fn print_all(state: tauri::State<'_, AppState>, order_id: i64) -> Result<String, String> {
    state.print_service.print_all(order_id)
}

/// 批量打印:多订单 × 多单据合并成一份分页 HTML(只弹一次打印对话框)
#[tauri::command]
fn print_batch(
    state: tauri::State<'_, AppState>,
    order_ids: Vec<i64>,
    doc_types: Vec<String>,
    screw_qty: Option<i64>,
) -> Result<String, String> {
    state
        .print_service
        .print_batch(&order_ids, &doc_types, screw_qty)
}

/// 螺丝规格规则列表(按门类型+型号系列,可在设置页编辑)
#[tauri::command]
fn screw_rule_list(state: tauri::State<'_, AppState>) -> Result<Vec<ScrewRule>, String> {
    let conn = state.print_service.db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT product_type, series, material_type, qty, spec, remark FROM screw_rules ORDER BY product_type, series, material_type")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ScrewRule {
                product_type: r.get(0)?,
                series: r.get(1)?,
                material_type: r.get(2)?,
                qty: r.get(3)?,
                spec: r.get(4)?,
                remark: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// 保存螺丝规格规则(数量/型号可随时改,方便以后多备几颗)
#[tauri::command]
fn screw_rule_save(
    state: tauri::State<'_, AppState>,
    rules: Vec<ScrewRule>,
) -> Result<(), String> {
    let mut conn = state.print_service.db.lock().unwrap();
    // 全量替换必须在一个事务里：否则中途失败会留下"半套"规则，且其它线程可能读到空表。
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM screw_rules", []).map_err(|e| e.to_string())?;
    for r in rules {
        tx.execute(
            "INSERT OR REPLACE INTO screw_rules(product_type,series,material_type,qty,spec,remark) VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![r.product_type, r.series, r.material_type, r.qty, r.spec, r.remark],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

// ---- 历史输入候选 ----

#[tauri::command]
fn history_list(state: tauri::State<'_, AppState>, field_key: String) -> Result<Vec<String>, String> {
    state.order_service.history_list(&field_key)
}

/// 某型号系列实际支持的轨道类型(选型号后自动填)
#[tauri::command]
fn series_tracks(state: tauri::State<'_, AppState>, series: String) -> Result<Vec<String>, String> {
    state.order_service.series_tracks(&series)
}

// ---- 草稿缓存(关闭前缓存未编完的新增订单,独立文件夹,7天自动清理) ----

fn draft_cleanup() {
    let dir = db::draft_dir();
    let now = chrono::Local::now().naive_local();
    let week = chrono::Duration::days(7);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if let Ok(md) = e.metadata() {
                if let Ok(m) = md.modified() {
                    let mt = chrono::DateTime::<chrono::Local>::from(m).naive_local();
                    if now - mt > week {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn draft_save(payload: String) -> Result<String, String> {
    draft_cleanup();
    let dir = db::draft_dir();
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S%3f").to_string();
    let name = format!("draft_{}.json", ts);
    std::fs::write(dir.join(&name), &payload).map_err(|e| e.to_string())?;
    Ok(name)
}

#[tauri::command]
fn draft_list() -> Result<Vec<DraftRecord>, String> {
    draft_cleanup();
    let dir = db::draft_dir();
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().map(|x| x == "json").unwrap_or(false) {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let mut order_no = String::new();
                let mut customer = String::new();
                let mut saved_at = None;
                let mut size = 0i64;
                if let Ok(s) = std::fs::read_to_string(&path) {
                    size = s.len() as i64;
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                        order_no = v
                            .get("order_no")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        customer = v
                            .get("customer")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                    }
                }
                if let Ok(md) = e.metadata() {
                    if let Ok(m) = md.modified() {
                        saved_at = Some(
                            chrono::DateTime::<chrono::Local>::from(m)
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string(),
                        );
                    }
                }
                out.push(DraftRecord {
                    name,
                    order_no,
                    customer,
                    saved_at,
                    size,
                });
            }
        }
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

/// 校验草稿文件名并解析为真实路径。
/// 原来的 `p.starts_with(&dir)` 只是词法比较，`dir/../data/app.db` 也能通过。
fn safe_draft_path(name: &str) -> Result<std::path::PathBuf, String> {
    if name.is_empty()
        || name.len() > 200
        || name.contains("..")
        || !name.ends_with(".json")
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err("非法草稿文件名".to_string());
    }
    let dir = db::draft_dir();
    let p = dir.join(name);
    let canon_dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let canon_p = p.canonicalize().map_err(|e| e.to_string())?;
    if !canon_p.starts_with(&canon_dir) {
        return Err("非法草稿文件名".to_string());
    }
    Ok(canon_p)
}

#[tauri::command]
fn draft_delete(name: String) -> Result<(), String> {
    let p = safe_draft_path(&name)?;
    std::fs::remove_file(p).map_err(|e| e.to_string())
}

#[tauri::command]
fn draft_get(name: String) -> Result<String, String> {
    let p = safe_draft_path(&name)?;
    std::fs::read_to_string(p).map_err(|e| e.to_string())
}

#[tauri::command]
fn draft_clear() -> Result<(), String> {
    let dir = db::draft_dir();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    Ok(())
}

/// 前端关闭守卫注册成功后调用；只有收到这个信号，后端才拦截窗口关闭。
#[tauri::command]
fn close_guard_ready() {
    CLOSE_GUARD_READY.store(true, Ordering::SeqCst);
}

/// 用户确认退出。先置放行标志（防止 app.exit 关闭窗口时又被 CloseRequested 拦截），
/// 清理单实例锁，再请求退出；另起一个看门狗，确保事件循环异常时进程也能结束。
#[tauri::command]
fn app_exit(app: tauri::AppHandle) {
    ALLOW_EXIT.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_file(db::single_instance_lock_path());
    app.exit(0);
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(3));
        std::process::exit(0);
    });
}

// ---- 导出 ----
#[tauri::command]
fn export_order(
    state: tauri::State<'_, AppState>,
    order_id: i64,
    format: String,
) -> Result<ExportResult, String> {
    // 导出前需已核对尺寸
    if !state.print_service.is_confirmed(order_id) {
        return Err("尺寸未核对,禁止导出。请先完成尺寸核对确认。".to_string());
    }
    state.export_service.export_order(order_id, &format)
}

#[tauri::command]
fn doc_type_label(doc_type: String) -> String {
    doc_type_name(&doc_type).to_string()
}

// ---- 订单管理页：一键导出 Excel（带勾选） ----
#[tauri::command]
fn export_orders_batch(
    state: tauri::State<'_, AppState>,
    order_ids: Vec<i64>,
    sections: Vec<String>,
) -> Result<ExportResult, String> {
    state.export_service.export_orders_batch(&order_ids, &sections)
}

/// 软件内直接查看表格：与导出的 xlsx 完全同源，不生成文件也能看
#[tauri::command]
fn export_orders_preview(
    state: tauri::State<'_, AppState>,
    order_ids: Vec<i64>,
    sections: Vec<String>,
) -> Result<Vec<export::SheetPreview>, String> {
    state.export_service.preview_orders_batch(&order_ids, &sections)
}

/// 校验分享文件路径：只允许本软件 data/export 目录内的真实文件。
/// 注意：Windows 的 canonicalize() 会返回 `\\?\D:\...` 这种 verbatim 前缀，
/// 直接交给 cmd start / explorer 会报「找不到网络路径」，所以校验用 canonicalize，
/// 返回给外部程序的必须是去掉前缀的普通路径。
fn checked_export_path(path: &str) -> Result<String, String> {
    let dir = db::data_dir().join("export");
    let canon_dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let canon = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !canon.starts_with(&canon_dir) || !canon.is_file() {
        return Err("只允许打开软件导出目录内的文件".to_string());
    }
    // 用 canonicalize 的结果去前缀（输入可能本来就不带前缀，用输入裁剪会漏掉）
    let s = canon.to_string_lossy().to_string();
    let clean = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    };
    Ok(clean)
}

/// 在资源管理器里定位导出的文件
#[tauri::command]
fn export_open_path(path: String) -> Result<(), String> {
    let p = checked_export_path(&path)?;
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", p))
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 用系统默认程序打开（xlsx → WPS/Excel，html → 浏览器），便于直接分享。
/// 直接调 ShellExecuteW（系统文件关联），**不经过 cmd /C start**：
/// 手动加引号 + Command 自己再转义一次，会把路径变成 `\D:\...\` 这种非法形式，
/// 用户看到的就是「Windows 找不到文件 \D:\...\htm\」。实现见 shell.rs。
#[tauri::command]
fn open_with_default(path: String) -> Result<(), String> {
    let p = checked_export_path(&path)?;
    shell::shell_open(&p)
}

/// 把导出文件放进剪贴板的文件列表（CF_HDROP）：
/// 切到微信 / QQ 聊天框按 Ctrl+V 就是把文件本身发出去，两个端都兼容。
#[tauri::command]
fn share_to_im(path: String) -> Result<String, String> {
    let p = checked_export_path(&path)?;
    let esc = p.replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; $c = New-Object System.Collections.Specialized.StringCollection; [void]$c.Add('{}'); [System.Windows.Forms.Clipboard]::SetFileDropList($c);",
        esc
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-WindowStyle", "Hidden", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok("文件已放入剪贴板：切到微信或 QQ 聊天框按 Ctrl+V 即可发送".to_string())
}

/// 切到微信 / QQ 的聊天窗口，方便粘贴文件。
/// **已在运行就只激活它的窗口，绝不启动新进程**——再启动一次客户端会要求扫码登录，
/// 这正是用户遇到的「点了打开微信，跳出来的是登录窗口」。
#[tauri::command]
fn open_im(which: String) -> Result<String, String> {
    let target = im::ImTarget::parse(&which).ok_or_else(|| "未知客户端".to_string())?;

    // 1) 已经在运行：直接把窗口叫到最前
    if im::focus_existing(target) {
        return Ok(format!(
            "已切到正在运行的{}，文件已在剪贴板，在聊天框按 Ctrl+V 发送",
            target.label()
        ));
    }
    // 2) 有进程但窗口没找到（例如缩在托盘里）：不要盲目再启动一次
    if im::is_running(target) {
        return Err(format!(
            "{}正在运行，但没找到它的窗口。请点任务栏右下角的托盘图标打开{}，再回来按 Ctrl+V。",
            target.label(),
            target.label()
        ));
    }
    // 3) 确实没在运行：才启动
    let exe = im::find(target).ok_or_else(|| {
        format!(
            "没找到{}的安装位置。请先手动打开{}，再回到这里按 Ctrl+V（文件已在剪贴板）",
            target.label(),
            target.label()
        )
    })?;
    let mut cmd = std::process::Command::new(&exe);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.spawn()
        .map_err(|e| format!("启动{}失败：{}（路径 {}）", target.label(), e, exe.display()))?;
    Ok(format!(
        "已打开{}（{}），登录后按 Ctrl+V 即可发送文件",
        target.label(),
        exe.display()
    ))
}

/// 打开 Windows「打印机和扫描仪」设置：把默认打印机的双面打印改成「单面」，
/// 只需设置一次，之后所有程序（含本软件）都会按单面出纸。
#[tauri::command]
fn open_printer_settings() -> Result<(), String> {
    shell::shell_open("ms-settings:printers")
}

/// 打开导出文件夹
#[tauri::command]
fn open_export_folder() -> Result<(), String> {
    let dir = db::data_dir().join("export");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    shell::shell_open(&dir.to_string_lossy())
}

/// 把多个文件一起放进剪贴板文件列表（微信/QQ 里 Ctrl+V 一次发多张图）
#[tauri::command]
fn share_files_to_im(paths: Vec<String>) -> Result<String, String> {
    let mut checked = Vec::new();
    for p in &paths {
        checked.push(checked_export_path(p)?);
    }
    if checked.is_empty() {
        return Err("没有可分享的文件".to_string());
    }
    let adds = checked
        .iter()
        .map(|p| format!("[void]$c.Add('{}');", p.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!(
        "Add-Type -AssemblyName System.Windows.Forms; $c = New-Object System.Collections.Specialized.StringCollection; {} [System.Windows.Forms.Clipboard]::SetFileDropList($c);",
        adds
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-WindowStyle", "Hidden", "-Command", &script])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(format!("{} 个文件已放入剪贴板，切到微信或 QQ 按 Ctrl+V 即可发送", checked.len()))
}

#[derive(serde::Serialize)]
struct MediaResult {
    files: Vec<String>,
    dir: String,
    count: usize,
}

/// 把勾选的订单 × 单据逐张渲染成 PNG（系统自带 Edge 无头截图，不需要 Office）
/// 导出进度事件：前端 listen('export-progress') 收，用来画进度条
#[derive(Clone, serde::Serialize)]
struct ExportProgress {
    done: usize,
    total: usize,
    tip: String,
}

/// 生成单据图片（逐张 Edge 截图）。
///
/// 两处关键设计：
/// 1. **3 路并行**：Edge 每张要 1 秒左右，串行导 35 张就是 40 秒，太难受；
///    现在 3 条线各用一个独立配置目录，互不抢锁，总时间降到 1/3 左右。
/// 2. **每张有超时**：Edge 偶尔会卡住，没有超时的话整个导出永远不结束。
/// 输出顺序与「单据类型 × 订单」的原始顺序一致（按任务下标回填），
/// 所以并发不会把图片顺序打乱。
pub(crate) fn render_share_images(
    svc: &PrintService,
    order_ids: &[i64],
    doc_types: &[String],
    out_dir: &std::path::Path,
    prefix: &str,
    on_progress: &(dyn Fn(usize, usize, &str) + Sync),
) -> Result<Vec<std::path::PathBuf>, String> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    let edge = media::find_edge()
        .ok_or_else(|| "没找到 Microsoft Edge，无法生成图片；可改用 Excel / HTML 导出".to_string())?;
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S%3f").to_string();

    // 任务表：按「单据类型 × 订单」展开，下标就是最终输出顺序
    let mut tasks: Vec<(&str, i64)> = Vec::with_capacity(doc_types.len() * order_ids.len());
    for dt in doc_types {
        for &id in order_ids {
            tasks.push((dt.as_str(), id));
        }
    }
    let total = tasks.len();
    if total == 0 {
        return Err("请先勾选订单和单据类型".to_string());
    }
    on_progress(0, total, "正在准备…");

    let workers = total.min(3).max(1);
    let done = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let first_err: Mutex<Option<String>> = Mutex::new(None);
    let out: Mutex<Vec<Option<std::path::PathBuf>>> = Mutex::new((0..total).map(|_| None).collect());

    std::thread::scope(|scope| {
        for slot in 0..workers {
            let done = &done;
            let stop = &stop;
            let first_err = &first_err;
            let out = &out;
            let tasks = &tasks;
            let edge = &edge;
            let stamp = &stamp;
            scope.spawn(move || {
                for i in (slot..total).step_by(workers) {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let (dt, id) = tasks[i];
                    let idx = i + 1;
                    let one = (|| -> Result<std::path::PathBuf, String> {
                        let html = svc.share_html(&[id], &vec![dt.to_string()])?;
                        // 截图专用：把内容约束到 900px 宽的竖版，字更大、留白更少
                        let html = html.replace(
                            "</style>",
                            "body{width:900px;margin:0 auto;background:#fff;padding:12px;} .share-doc{padding:4px 0 12px;}</style>",
                        );
                        let hp = out_dir.join(format!(".tmp_{}_{}.html", stamp, idx));
                        let png = out_dir.join(format!("{}_{}_{}.png", prefix, stamp, idx));
                        std::fs::write(&hp, html).map_err(|e| e.to_string())?;
                        let r = media::html_to_png(edge, &hp, &png, idx, slot);
                        let _ = std::fs::remove_file(&hp);
                        r?;
                        Ok(png)
                    })();
                    match one {
                        Ok(png) => {
                            out.lock().unwrap()[i] = Some(png);
                            let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                            on_progress(
                                d,
                                total,
                                &format!("已完成 {} / {} 张（刚做完：{}）", d, total, doc_type_name(dt)),
                            );
                        }
                        Err(e) => {
                            stop.store(true, Ordering::SeqCst);
                            let mut fe = first_err.lock().unwrap();
                            if fe.is_none() {
                                *fe = Some(e);
                            }
                            break;
                        }
                    }
                }
            });
        }
    });

    if let Some(e) = first_err.lock().unwrap().clone() {
        return Err(e);
    }
    let collected = out.lock().unwrap();
    Ok(collected.iter().filter_map(|x| x.clone()).collect())
}

/// 同一时间只允许一个导出任务：两批同时截图会各自开一堆 Edge，
/// 互相抢配置目录，反而更慢更容易卡住。
static EXPORTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct ExportGuard;
impl Drop for ExportGuard {
    fn drop(&mut self) {
        EXPORTING.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
fn begin_export() -> Result<ExportGuard, String> {
    EXPORTING
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .map(|_| ExportGuard)
        .map_err(|_| "已经在导出中，等这一次完成再点".to_string())
}

#[tauri::command]
async fn export_images(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    order_ids: Vec<i64>,
    doc_types: Vec<String>,
) -> Result<MediaResult, String> {
    let _guard = begin_export()?;
    let svc = state.print_service.clone();
    let dir = db::data_dir().join("export");
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |done: usize, total: usize, tip: &str| {
            let _ = app.emit(
                "export-progress",
                ExportProgress {
                    done,
                    total,
                    tip: tip.to_string(),
                },
            );
        };
        let imgs = render_share_images(&svc, &order_ids, &doc_types, &dir, "单据图片", &emit)?;
        Ok(MediaResult {
            count: imgs.len(),
            files: imgs.iter().map(|p| p.to_string_lossy().to_string()).collect(),
            dir: dir.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|e| format!("导出线程异常：{}", e))?
}

#[tauri::command]
async fn export_pptx(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    order_ids: Vec<i64>,
    doc_types: Vec<String>,
) -> Result<ExportResult, String> {
    let _guard = begin_export()?;
    let svc = state.print_service.clone();
    let dir = db::data_dir().join("export");
    tauri::async_runtime::spawn_blocking(move || {
        let tmp = dir.join(format!(
            ".tmp_ppt_{}",
            chrono::Local::now().format("%Y%m%d_%H%M%S%3f")
        ));
        let emit = |done: usize, total: usize, tip: &str| {
            let _ = app.emit(
                "export-progress",
                ExportProgress {
                    done,
                    total,
                    tip: tip.to_string(),
                },
            );
        };
        let imgs = render_share_images(&svc, &order_ids, &doc_types, &tmp, "页", &emit)?;
        let filename = format!(
            "客户单据_{}.pptx",
            chrono::Local::now().format("%Y%m%d_%H%M%S%3f")
        );
        let path = dir.join(&filename);
        let r = media::build_pptx(&imgs, &path);
        let _ = std::fs::remove_dir_all(&tmp);
        r?;
        Ok(ExportResult {
            path: path.to_string_lossy().to_string(),
            filename,
        })
    })
    .await
    .map_err(|e| format!("导出线程异常：{}", e))?
}

/// 生成「客户单据」HTML：把勾选订单按勾选的单据样板渲染成一份可直接看/分享的文件
#[tauri::command]
fn share_docs(
    state: tauri::State<'_, AppState>,
    order_ids: Vec<i64>,
    doc_types: Vec<String>,
) -> Result<ExportResult, String> {
    let html = state.print_service.share_html(&order_ids, &doc_types)?;
    let dir = db::data_dir().join("export");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let filename = format!("客户单据_{}.html", chrono::Local::now().format("%Y%m%d_%H%M%S%3f"));
    let path = dir.join(&filename);
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    Ok(ExportResult {
        path: path.to_string_lossy().to_string(),
        filename,
    })
}

// ---- 备份：目录大小 / 整库恢复 ----
#[tauri::command]
fn backup_manual_size(state: tauri::State<'_, AppState>) -> i64 {
    state.backup_service.manual_dir_size()
}

#[tauri::command]
fn backup_restore_prepare(
    state: tauri::State<'_, AppState>,
    ty: Option<String>,
    name: String,
) -> Result<String, String> {
    state
        .backup_service
        .prepare_restore(ty.as_deref().unwrap_or(""), &name)
}

// ---- 未提交表单快照（定时 / 关闭前；只存表单，不存整库） ----
#[derive(serde::Serialize)]
struct FormSnapshot {
    id: i64,
    kind: String,
    created_at: String,
    size: i64,
    preview: String,
}

#[tauri::command]
fn form_snapshot_save(
    state: tauri::State<'_, AppState>,
    kind: String,
    payload: String,
) -> Result<i64, String> {
    if kind != "auto" && kind != "shutdown" {
        return Err("未知快照类型".to_string());
    }
    // 硬性边界：payload 只能是表单 JSON，禁止混入整库数据
    let v: serde_json::Value = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
    if !v.is_object() || v.get("order_form").is_none() && v.get("contract_form").is_none() {
        return Err("快照内容必须是未提交表单数据".to_string());
    }
    let conn = state.order_service.db.lock().unwrap();
    conn.execute(
        "INSERT INTO form_snapshots(kind,payload) VALUES(?1,?2)",
        rusqlite::params![kind, payload],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    let limit: i64 = if kind == "auto" { 5 } else { 3 };
    // 只保留最新 limit 条，超出自动删最旧
    conn.execute(
        "DELETE FROM form_snapshots WHERE kind=?1 AND id NOT IN \
         (SELECT id FROM form_snapshots WHERE kind=?1 ORDER BY id DESC LIMIT ?2)",
        rusqlite::params![kind, limit],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
fn form_snapshot_list(
    state: tauri::State<'_, AppState>,
    kind: String,
) -> Result<Vec<FormSnapshot>, String> {
    let conn = state.order_service.db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id,kind,created_at,length(payload),substr(payload,1,120) FROM form_snapshots WHERE kind=?1 ORDER BY id DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![kind], |r| {
            Ok(FormSnapshot {
                id: r.get(0)?,
                kind: r.get(1)?,
                created_at: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                size: r.get(3)?,
                preview: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
fn form_snapshot_get(state: tauri::State<'_, AppState>, id: i64) -> Result<String, String> {
    let conn = state.order_service.db.lock().unwrap();
    conn.query_row("SELECT payload FROM form_snapshots WHERE id=?1", rusqlite::params![id], |r| {
        r.get::<_, String>(0)
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn form_snapshot_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let conn = state.order_service.db.lock().unwrap();
    conn.execute("DELETE FROM form_snapshots WHERE id=?1", rusqlite::params![id])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn form_snapshot_clear(state: tauri::State<'_, AppState>, kind: String) -> Result<(), String> {
    let conn = state.order_service.db.lock().unwrap();
    conn.execute("DELETE FROM form_snapshots WHERE kind=?1", rusqlite::params![kind])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 启动时执行上一次请求的整库恢复：先保留现库，再原子替换，然后删除 WAL/SHM 与标记。
fn apply_pending_restore() {
    let d = db::data_dir();
    let flag = d.join("restore.flag");
    if !flag.exists() {
        return;
    }
    let pending = d.join("restore_pending.sqlite");
    let target = d.join("app.db");
    if pending.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let _ = std::fs::copy(&target, d.join(format!("app_before_restore_{}.db", stamp)));
        if std::fs::rename(&pending, &target).is_ok() {
            let _ = std::fs::remove_file(d.join("app.db-wal"));
            let _ = std::fs::remove_file(d.join("app.db-shm"));
        }
    }
    let _ = std::fs::remove_file(&flag);
}

fn main() {
    // 若用户上次请求了整库恢复，先完成替换再打开数据库
    apply_pending_restore();

    // 单实例必须在数据库初始化之前抢锁：
    // ① 重复双击能立刻切回已有窗口，不用等上千条公式补种；
    // ② 两个进程同时启动时，不会都跑完初始化再互相抢，出现「两个进程都没窗口」。
    if allow_multi_open_fast() {
        let _ = std::fs::remove_file(db::single_instance_lock_path());
    } else if try_acquire_single_instance().is_some() {
        let mut activated = single::activate_existing();
        if !activated {
            // 对方可能刚启动、窗口还没建好，最多等 4 秒
            for _ in 0..16 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if single::activate_existing() {
                    activated = true;
                    break;
                }
            }
        }
        if !activated {
            show_already_running();
        }
        std::process::exit(0);
    }

    // 初始化数据库(落在软件目录 data/ 下,D 盘)
    let conn: Connection = db::init();
    let conn = Arc::new(Mutex::new(conn));

    // 插件宿主
    let app_dir = db::app_dir();
    let plugin_host = Arc::new(PluginHost::new(&app_dir));
    plugin_host.load_all();

    // 服务装配
    let order_service = order::OrderService::new(conn.clone(), plugin_host.clone());

    // 历史订单补录客户：旧版本保存订单时没同步客户表，客户管理里是空的。
    // 只跑一次（跑完写标记），避免用户手动删掉的客户下次启动又被订单「复活」。
    {
        let c = conn.lock().unwrap();
        if db::get_config(&c, "app.customers_backfilled").as_deref() != Some("1") {
            match order::backfill_customers_from_orders(&c) {
                Ok(n) => {
                    let _ = db::set_config(&c, "app.customers_backfilled", "1");
                    if n > 0 {
                        eprintln!("已从历史订单补录 {} 位客户到客户管理", n);
                    }
                }
                Err(e) => eprintln!("补录客户失败：{}", e),
            }
        }
    }
    let backup_service = backup::BackupService::new(conn.clone());
    // 旧版本「删除备份」走软删除（移入备份目录下的 .trash 回收区），文件其实还在磁盘上。
    // 用户要求删就是真删：启动时把各备份目录里残留的 .trash 直接永久删除。
    for ty in backup::BACKUP_TYPES {
        let dir = backup_service.cfg(ty).0;
        let trash = dir.join(".trash");
        if trash.is_dir() {
            match std::fs::remove_dir_all(&trash) {
                Ok(_) => eprintln!("已永久清理残留回收区: {}", trash.display()),
                Err(e) => eprintln!("清理回收区失败 {}: {}", trash.display(), e),
            }
        }
    }
    // WebView2 的崩溃转储会随异常退出累积（实测涨到 40MB+），启动时清掉，
    // 只删 reports/pending 里的 dump 文件，不碰登录数据所在的 Default 目录。
    if let Some(local) = dirs::data_local_dir() {
        let cp = local
            .join("com.example.doorwindow")
            .join("EBWebView")
            .join("Crashpad");
        for sub in ["reports", "pending", "completed"] {
            let d = cp.join(sub);
            if d.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&d) {
                    let mut n = 0u32;
                    for f in rd.flatten() {
                        if f.path().is_file() && std::fs::remove_file(f.path()).is_ok() {
                            n += 1;
                        }
                    }
                    if n > 0 {
                        eprintln!("已清理崩溃转储 {} 个: {}", n, d.display());
                    }
                }
            }
        }
    }
    let print_service = PrintService::new(conn.clone(), plugin_host.clone());
    let export_service = ExportService::new(conn.clone());

    let state = AppState {
        order_service,
        backup_service,
        print_service,
        export_service,
        plugin_host,
    };

    // 启动 6 秒后（用户已经在用了）后台预热 Edge 截图配置目录，
    // 这样第一次导出图片不用再等 5–6 秒的初始化。失败也不影响任何功能。
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(6));
        media::warm_up_profiles(3);
    });

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            meta,
            set_config,
            get_config,
            config_list,
            order_list,
            order_get,
            order_create,
            order_clear_details,
            render_preview,
            order_update,
            order_set_status,
            order_remove,
            customer_list,
            customer_save,
            customer_remove,
            contract_list,
            contract_save,
            contract_remove,
            component_list,
            component_add,
            component_remove,
            component_descendants,
            profile_list,
            profile_save,
            profile_remove,
            glass_list,
            glass_save,
            glass_remove,
            accessory_list,
            accessory_save,
            accessory_remove,
            step_list,
            step_toggle,
            preset_list,
            preset_save,
            preset_remove,
            glass_type_list,
            profile_lib_list,
            profile_lib_types,
            profile_lib_save,
            profile_lib_names,
            stats_overview,
            stats_customers,
            calc_profile_sizes,
            calc_glass_sizes,
            plugin_list,
            plugin_enable,
            plugin_uninstall,
            plugin_install_demo,
            backup_list,
            backup_run,
            backup_delete,
            backup_clear,
            print_check_detail,
            print_check_page,
            print_confirm,
            print_run,
            print_all,
            print_batch,
            screw_rule_list,
            screw_rule_save,
            history_list,
            series_tracks,
            draft_save,
            draft_list,
            draft_delete,
            draft_get,
            draft_clear,
            close_guard_ready,
            app_exit,
            export_order,
            export_orders_batch,
            export_orders_preview,
            export_open_path,
            open_with_default,
            share_to_im,
            open_im,
            open_printer_settings,
            open_export_folder,
            share_files_to_im,
            export_images,
            export_pptx,
            share_docs,
            backup_manual_size,
            backup_restore_prepare,
            form_snapshot_save,
            form_snapshot_list,
            form_snapshot_get,
            form_snapshot_delete,
            form_snapshot_clear,
            doc_type_label,
        ])
        .on_window_event(|window, event| {
            // 关闭窗口前：交给前端弹「是否确认关闭」并先缓存未编完的草稿。
            // 关键：必须有多重放行条件，否则一旦前端收不到事件，窗口会被永久钉死。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 1) 用户已确认退出：直接放行，不再拦截（也兼容 app.exit 关闭窗口的路径）
                if ALLOW_EXIT.load(Ordering::SeqCst) {
                    return;
                }
                // 2) 前端守卫没注册成功（事件权限缺失 / JS 报错）：不拦截，保证至少能关掉
                if !CLOSE_GUARD_READY.load(Ordering::SeqCst) {
                    ALLOW_EXIT.store(true, Ordering::SeqCst);
                    let _ = std::fs::remove_file(db::single_instance_lock_path());
                    return;
                }
                api.prevent_close();
                // 3) 事件发送失败：不能让窗口卡死，直接请求退出
                if window.emit("request-close", ()).is_err() {
                    ALLOW_EXIT.store(true, Ordering::SeqCst);
                    let app = window.app_handle().clone();
                    std::thread::spawn(move || app.exit(0));
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            match event {
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
                    let _ = std::fs::remove_file(db::single_instance_lock_path());
                }
                _ => {}
            }
        });
}

/// 启动最早期判断是否允许多开：只读打开库读一行配置，不跑 seed（毫秒级）
fn allow_multi_open_fast() -> bool {
    let p = db::data_dir().join("app.db");
    if !p.exists() {
        return false;
    }
    match Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(c) => c
            .query_row(
                "SELECT value FROM app_config WHERE key='app.allow_multi_open'",
                [],
                |r| r.get::<_, String>(0),
            )
            .map(|v| v == "true")
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// 抢单实例锁（纯文件操作，不依赖数据库）。
/// 返回 `Some(pid)` 表示已有实例在跑，`pid` 是它的进程号；返回 `None` 表示本进程拿到了锁。
fn try_acquire_single_instance() -> Option<u32> {
    use std::io::Write;
    let lock = db::single_instance_lock_path();
    if let Some(d) = lock.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    for _ in 0..3 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut f) => {
                let _ = writeln!(f, "{}", std::process::id());
                return None;
            }
            Err(_) => {
                let pid = std::fs::read_to_string(&lock)
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok());
                if let Some(p) = pid {
                    if p != std::process::id() && process_alive(p) {
                        return Some(p);
                    }
                }
                // 残留锁 / 自己写的旧锁：清掉再试一次
                let _ = std::fs::remove_file(&lock);
            }
        }
    }
    None
}

/// 精确判断 PID 是否存活。原来用 `stdout.contains(pid)` 会被内存列的数字
/// （如 `12,345 K`）误命中，导致残留锁被当成"已在运行"，软件再也打不开。
fn pid_mentioned_in_tasklist(text: &str, pid: u32) -> bool {
    let pid_s = pid.to_string();
    text.lines()
        .any(|line| line.split_whitespace().any(|token| token == pid_s))
}

fn process_alive(pid: u32) -> bool {
    match std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
    {
        Ok(o) => pid_mentioned_in_tasklist(&String::from_utf8_lossy(&o.stdout), pid),
        Err(_) => false,
    }
}

/// 激活失败时的兜底提示：用系统原生 MessageBox，不启动 PowerShell
/// （原来启 PowerShell 弹框，主进程会一直等它，用户看到的就是「多了一个没有窗口的进程」）
#[cfg(windows)]
fn show_already_running() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONINFORMATION, MB_OK, MB_TOPMOST,
    };
    let text: Vec<u16> = "门窗管理软件已经在运行，但没能把它的窗口调到前面。\n请在任务栏点一下它的图标。"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let title: Vec<u16> = "提示"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MessageBoxW(0, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONINFORMATION | MB_TOPMOST);
    }
}

#[cfg(not(windows))]
fn show_already_running() {}
