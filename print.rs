// src-tauri/src/print.rs
// 打印调度 + 尺寸核对门禁 + 导出(Rust 版)
// 合规说明:不实现去除/替换第三方标识;模板默认不显示第三方标识,
// 支持"添加自定义图标"或"不添加图标";计算规则不在此模块自动匹配。

use crate::model::{CheckDetail, CheckRow, Component, Glass, Profile};
use crate::plugin::PluginHost;
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub const DOC_TYPES: [&str; 7] = ["cutting", "glass_cutting", "certificate", "delivery", "sales", "receipt", "label"];

/// 渲染选项:批量打印时传入页码与螺丝数量临时覆盖值
#[derive(Clone, Default)]
pub struct RenderOpts {
    pub screw_qty: Option<i64>,
    pub page_no: usize,
    pub page_total: usize,
}

impl RenderOpts {
    /// 模板内页码:单张打印显示;批量打印留空(由合并层统一页眉显示,避免重复)
    pub fn page_info(&self) -> String {
        if self.page_total > 1 {
            String::new()
        } else {
            "第 1 页 共 1 页".to_string()
        }
    }
    /// 合并层每页页眉的页码
    pub fn page_label(&self) -> String {
        if self.page_total > 1 {
            format!("第 {} 页 共 {} 页", self.page_no, self.page_total)
        } else {
            "第 1 页 共 1 页".to_string()
        }
    }
}

/// 打印页眉里的订单号/厂名来自用户输入，拼进 HTML 前必须转义
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn doc_type_name(t: &str) -> &'static str {
    match t {
        "cutting" => "下料单",
        "glass_cutting" => "玻璃下料单",
        "certificate" => "合格证",
        "delivery" => "发货清单",
        "sales" => "销售单",
        "receipt" => "回执单",
        "label" => "标签",
        _ => "未知",
    }
}

/// 会被 clone 一份丢进后台线程（导出图片时不能占着主线程），
/// 所以内部全部用 Arc 共享同一份连接/插件/核对记录。
#[derive(Clone)]
pub struct PrintService {
    pub db: Arc<Mutex<Connection>>,
    pub plugin_host: std::sync::Arc<PluginHost>,
    confirmed: Arc<Mutex<HashSet<i64>>>,
}

impl PrintService {
    pub fn new(db: Arc<Mutex<Connection>>, plugin_host: std::sync::Arc<PluginHost>) -> Self {
        PrintService {
            db,
            plugin_host,
            confirmed: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// 完整核对信息:玻璃尺寸、总宽高、型材、产品、玻璃类型
    pub fn build_check_detail(&self, order_id: i64) -> Result<CheckDetail, String> {
        let conn = self.db.lock().unwrap();

        let mut detail = CheckDetail {
            order_id,
            ..Default::default()
        };

        // 订单主字段
        if let Ok(row) = conn.query_row(
            "SELECT order_no, customer, width, height, glass_layer, product_name, \
             material_type, style, color, face_glass, back_glass, face_glass_w, face_glass_h, \
             back_glass_w, back_glass_h FROM orders WHERE id = ?1",
            params![order_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<String>>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, Option<f64>>(11)?,
                    r.get::<_, Option<f64>>(12)?,
                    r.get::<_, Option<f64>>(13)?,
                    r.get::<_, Option<f64>>(14)?,
                ))
            },
        ) {
            detail.order_no = row.0.unwrap_or_default();
            detail.customer = row.1;
            detail.width = row.2;
            detail.height = row.3;
            detail.glass_layer = row.4;
            detail.product_name = row.5;
            detail.material_type = row.6;
            detail.style = row.7;
            detail.color = row.8;
            detail.face_glass = row.9;
            detail.back_glass = row.10;
            detail.face_glass_w = row.11;
            detail.face_glass_h = row.12;
            detail.back_glass_w = row.13;
            detail.back_glass_h = row.14;
        }

        // 型材明细
        detail.profiles = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, order_id, profile_name, length, quantity, category, remark \
                     FROM profiles WHERE order_id = ?1 ORDER BY id",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![order_id], |r| {
                    Ok(Profile {
                        id: r.get(0)?,
                        order_id: r.get(1)?,
                        profile_name: r.get(2)?,
                        length: r.get(3)?,
                        quantity: r.get(4)?,
                        category: r.get(5)?,
                        remark: r.get(6)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };

        // 玻璃明细
        detail.glasses = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, order_id, glass_name, width, height, quantity, remark \
                     FROM glasses WHERE order_id = ?1 ORDER BY id",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![order_id], |r| {
                    Ok(Glass {
                        id: r.get(0)?,
                        order_id: r.get(1)?,
                        glass_name: r.get(2)?,
                        width: r.get(3)?,
                        height: r.get(4)?,
                        quantity: r.get(5)?,
                        remark: r.get(6)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };

        drop(conn);
        Ok(detail)
    }

    /// 尺寸核对页数据:列出原始/实际尺寸、型材、数量,标记异常
    pub fn build_check_page(&self, order_id: i64) -> Result<Vec<CheckRow>, String> {
        let comps = self.list_components_raw(order_id)?;
        Ok(comps
            .into_iter()
            .map(|comp| {
                let ow = comp.orig_width.unwrap_or(0.0);
                let oh = comp.orig_height.unwrap_or(0.0);
                let aw = comp.actual_width.unwrap_or(0.0);
                let ah = comp.actual_height.unwrap_or(0.0);
                let abnormal = (ow - aw).abs() > 1e-6 || (oh - ah).abs() > 1e-6 || comp.quantity <= 0;
                CheckRow { comp, abnormal }
            })
            .collect())
    }

    fn list_components_raw(&self, order_id: i64) -> Result<Vec<Component>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, order_id, parent_id, path, name, spec, material_type, \
                 orig_width, orig_height, actual_width, actual_height, quantity, sort_order \
                 FROM components WHERE order_id = ?1 ORDER BY sort_order, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![order_id], |row| {
                Ok(Component {
                    id: row.get(0)?,
                    order_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    path: row.get(3)?,
                    name: row.get(4)?,
                    spec: row.get(5)?,
                    material_type: row.get(6)?,
                    orig_width: row.get(7)?,
                    orig_height: row.get(8)?,
                    actual_width: row.get(9)?,
                    actual_height: row.get(10)?,
                    quantity: row.get(11)?,
                    sort_order: row.get(12)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// 尺寸确认:显式勾选后记录日志 + 触发 Hook + 自动将订单状态置为「已审核」
    pub fn confirm_size(&self, order_id: i64, operator: &str) -> Result<(), String> {
        let raw = serde_json::json!({ "orderId": order_id, "operator": operator }).to_string();
        {
            let conn = self.db.lock().unwrap();
            conn.execute(
                "INSERT INTO size_confirm_log(order_id, operator, raw_json) VALUES(?1,?2,?3)",
                params![order_id, operator, raw],
            )
            .map_err(|e| e.to_string())?;
            // 只在「未合并」时前移到「已审核」。
            // 原来是无条件 UPDATE，导致已排产/已出库的订单只要打印一次就被打回「已审核」，
            // 生产台账直接倒退。
            conn.execute(
                "UPDATE orders SET status='已审核', updated_at=datetime('now','localtime') \
                 WHERE id=?1 AND status='未合并'",
                params![order_id],
            )
            .map_err(|e| e.to_string())?;
        }

        let payload = serde_json::json!({
            "api_version": "1.0",
            "hook": "size.confirmed",
            "payload": { "orderId": order_id, "confirmedBy": operator }
        });
        self.plugin_host.dispatch("size.confirmed", &payload)?;

        self.confirmed.lock().unwrap().insert(order_id);
        Ok(())
    }

    /// 取该型号在指定玻璃层数下的开孔参数（只有平开门的 glass_holes 有数据，其它型号自然返回 —）。
    /// 右分支截图被截断未录入时不推测，只显示已确认的左分支。
    fn glass_hole_text(&self, series: &str) -> String {
        let conn = self.db.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT DISTINCT hole_left FROM glass_holes \
             WHERE series=?1 AND hole_left IS NOT NULL ORDER BY id",
        ) {
            Ok(s) => s,
            Err(_) => return "—".to_string(),
        };
        let vals: Vec<String> = stmt
            .query_map(params![series], |r| r.get::<_, String>(0))
            .ok()
            .map(|rows| rows.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();
        if vals.is_empty() {
            "—".to_string()
        } else {
            vals.iter()
                .map(|s| html_escape(s))
                .collect::<Vec<_>>()
                .join("<br>")
        }
    }

    /// 是否已完成尺寸核对。原来只查内存 HashSet，软件一重启所有订单都变成
    /// "未核对"，打印/导出全被拒；核对记录本来就已写入 size_confirm_log，这里回查数据库。
    pub fn is_confirmed(&self, order_id: i64) -> bool {
        if self.confirmed.lock().unwrap().contains(&order_id) {
            return true;
        }
        let persisted = {
            let conn = self.db.lock().unwrap();
            conn.query_row(
                "SELECT 1 FROM size_confirm_log WHERE order_id = ?1 LIMIT 1",
                params![order_id],
                |_| Ok(()),
            )
            .is_ok()
        };
        if persisted {
            self.confirmed.lock().unwrap().insert(order_id);
        }
        persisted
    }

    /// 打印/导出主入口:未核对则拒绝
    pub fn print(&self, order_id: i64, doc_type: &str, output: &str) -> Result<String, String> {
        if !self.is_confirmed(order_id) {
            return Err("尺寸未核对,禁止打印。请先完成尺寸核对确认。".to_string());
        }
        if !DOC_TYPES.contains(&doc_type) {
            return Err(format!("未知文档类型: {}", doc_type));
        }

        // print.before Hook(可中止)
        let before = serde_json::json!({
            "api_version": "1.0",
            "hook": "print.before",
            "payload": { "docType": doc_type, "orderId": order_id }
        });
        self.plugin_host.dispatch("print.before", &before)?;

        // 渲染
        let html = self.render(order_id, doc_type, &RenderOpts::default())?;

        // 记录打印日志 + print.after
        {
            let conn = self.db.lock().unwrap();
            conn.execute(
                "INSERT INTO print_log(order_id, doc_type, template_version) VALUES(?1,?2,'1.0')",
                params![order_id, doc_type],
            )
            .map_err(|e| e.to_string())?;
        }
        let after = serde_json::json!({
            "api_version": "1.0",
            "hook": "print.after",
            "payload": { "docType": doc_type, "orderId": order_id }
        });
        self.plugin_host.dispatch("print.after", &after)?;

        let _ = output;
        Ok(html)
    }

    /// 查询该订单适用的螺丝规格:优先按「型号 + 该订单料型」精确匹配,
    /// 找不到再回落到该型号的「通用」规则;数量可被打印弹窗临时覆盖。
    fn screw_rule(&self, series: &str, material_type: &str, override_qty: Option<i64>) -> (i64, String) {
        let conn = self.db.lock().unwrap();
        let mut pt = String::new();
        if let Ok(v) = conn.query_row(
            "SELECT product_type FROM profile_lib WHERE series=?1 LIMIT 1",
            params![series],
            |r| r.get::<_, String>(0),
        ) {
            pt = v;
        }
        let (mut qty, mut spec): (i64, String) =
            (24, "ST4.2×25 304不锈钢自攻螺丝".to_string());
        let mut matched: Option<(i64, String)> = None;
        if !material_type.is_empty() {
            matched = conn
                .query_row(
                    "SELECT qty, spec FROM screw_rules WHERE series=?1 AND material_type=?2 LIMIT 1",
                    params![series, material_type],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
                )
                .ok();
        }
        if matched.is_none() {
            matched = conn
                .query_row(
                    "SELECT qty, spec FROM screw_rules WHERE series=?1 AND material_type='通用' LIMIT 1",
                    params![series],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
                )
                .ok();
        }
        if matched.is_none() {
            matched = conn
                .query_row(
                    "SELECT qty, spec FROM screw_rules WHERE series=?1 LIMIT 1",
                    params![series],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
                )
                .ok();
        }
        if let Some(row) = matched {
            qty = row.0;
            spec = row.1;
        } else if pt == "推拉门" {
            qty = 36;
        }
        drop(conn);
        if let Some(q) = override_qty {
            qty = q;
        }
        (qty.max(0), spec)
    }

    /// 一键打印单订单的全部单据
    pub fn print_all(&self, order_id: i64) -> Result<String, String> {
        let all = DOC_TYPES.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        self.print_batch(&[order_id], &all, None)
    }

    /// 批量打印:多订单 × 多单据,合并为一份分页 HTML,只调起一次打印对话框。
    /// 规则:不同单据类型分纸(不放同一张);同一种单据**每页最多 3 单、从上往下纵向排**,
    /// 每单都占满整页宽度,不再并排挤成 3 个窄列;块与块之间用订单号小标题分隔,方便看单。
    /// 每个订单必须先完成尺寸核对(沿用原有打印门禁)。
    pub fn print_batch(
        &self,
        order_ids: &[i64],
        doc_types: &[String],
        screw_qty: Option<i64>,
    ) -> Result<String, String> {
        if order_ids.is_empty() || doc_types.is_empty() {
            return Err("请先选择订单和打印单据".to_string());
        }
        for &id in order_ids {
            if !self.is_confirmed(id) {
                return Err(format!("订单 {} 尺寸未核对，禁止打印。请先勾选「已核对」。", id));
            }
        }
        for dt in doc_types {
            if !DOC_TYPES.contains(&dt.as_str()) {
                return Err(format!("未知文档类型: {}", dt));
            }
        }
        let company_name: String = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT value FROM app_config WHERE key='company.name'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_default()
            .trim()
            .to_string()
        };

        // 1) 渲染所有 (单据类型, 订单号, 单据body),顺序:按单据类型分组
        let mut rendered: Vec<(String, String, String)> = Vec::new();
        for dt in doc_types {
            for &id in order_ids {
                let opts = RenderOpts {
                    screw_qty,
                    page_no: 0,
                    page_total: 0,
                };
                let full = self.render(id, dt, &opts)?;
                let body = extract_body(&full);
                let order_no: String = {
                    let c = self.db.lock().unwrap();
                    c.query_row(
                        "SELECT order_no FROM orders WHERE id=?1",
                        params![id],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap_or_default()
                };
                rendered.push((dt.clone(), order_no, body));
                if let Ok(conn) = self.db.lock() {
                    conn.execute(
                        "INSERT INTO print_log(order_id,doc_type,template_version) VALUES(?1,?2,'1.0')",
                        params![id, dt],
                    )
                    .ok();
                }
            }
        }

        // 2) 按单据类型分组，每组每页最多 3 单。
        //    同一页里 3 单是「从上往下」依次排列，不是并排 N 列。
        let mut sheets: Vec<(String, Vec<String>, Vec<String>)> = Vec::new(); // (dt, order_nos, bodies)
        let mut i = 0usize;
        while i < rendered.len() {
            let dt = rendered[i].0.clone();
            let mut nos = Vec::new();
            let mut bodies = Vec::new();
            while i < rendered.len() && rendered[i].0 == dt && bodies.len() < 3 {
                nos.push(rendered[i].1.clone());
                bodies.push(rendered[i].2.clone());
                i += 1;
            }
            sheets.push((dt, nos, bodies));
        }
        let total_sheets = sheets.len();
        let company_esc = html_escape(&company_name);

        // 3) 拼 HTML：每页最多 3 单，纵向堆叠；每单一个满宽区块 + 订单号小标题
        let mut parts: Vec<String> = Vec::new();
        for (idx, (dt, nos, bodies)) in sheets.into_iter().enumerate() {
            let page_no = idx + 1;
            let order_nos = nos
                .iter()
                .map(|n| html_escape(n))
                .collect::<Vec<_>>()
                .join("、");
            let blocks = nos
                .iter()
                .zip(bodies.into_iter())
                .map(|(no, b)| {
                    format!(
                        "<div class=\"order-block\"><div class=\"order-band\">订单 {}</div><div class=\"order-body\">{}</div></div>",
                        html_escape(no), b
                    )
                })
                .collect::<Vec<_>>()
                .join("");
            parts.push(format!(
                "<section class=\"sheet\"><div class=\"batch-page-info\"><span class=\"pi-page\">第 {page_no} 页 共 {total_sheets} 页</span><span class=\"pi-doc\">{doc}</span><span class=\"pi-order\">订单 {order_nos}</span><span class=\"pi-company\">{company}</span></div><div class=\"sheet-body\">{blocks}</div></section>",
                page_no = page_no,
                total_sheets = total_sheets,
                doc = doc_type_name(dt.as_str()),
                order_nos = order_nos,
                company = company_esc,
                blocks = blocks,
            ));
        }
        // 单订单打印走正常字号；只有多单才注入压缩 CSS（一页塞 3 单的代价）
        let compact_css: String = if order_ids.len() > 1 {
            r#"
  /* 批量打印压缩：一页要放 3 个订单，字号/间距大幅收紧 */
  .sheet-body .head{{margin-bottom:2px;gap:5px;}}
  .sheet-body .logo{{width:14px;height:14px;line-height:14px;font-size:8px;}}
  .sheet-body h1{{font-size:13px;letter-spacing:1px;}}
  .sheet-body .head .dt{{display:none;}}
  .sheet-body .sub{{margin-bottom:4px;}}
  .sheet-body .info{{margin-bottom:4px;font-size:11px;gap:3px 12px;line-height:1.35;}}
  .sheet-body .block{{padding:3px 6px;margin-bottom:4px;font-size:10.5px;line-height:1.3;}}
  .sheet-body th,.sheet-body td{{padding:2px 4px;font-size:10.5px;}}
  .sheet-body td.k{{width:40px;}}
  .sheet-body .prod{{font-size:11px;}}
  .sheet-body .m-title{{margin:3px 0 2px;font-size:11px;}}
  .sheet-body .sign{{margin-top:5px;}}
  .sheet-body .note{{margin-top:3px;}}
  .sheet-body .notice{{margin-top:5px;padding-top:3px;line-height:1.45;}}
  .sheet-body .total{{margin-top:4px;}}
  .sheet-body .customer-bar{{margin:3px 0 2px;}}
  .sheet-body .screw{{margin-top:3px;padding-top:2px;font-size:10px;}}
  .sheet-body .screw .st{{margin-bottom:1px;}}
  .sheet-body .screw .sl{{line-height:1.4;}}
  .sheet-body .gline{{margin:1px 0;font-size:10.5px;}}
  .sheet-body .gline .gl{{width:46px;}}
  .sheet-body .order-band{{padding:2px 8px;margin-bottom:3px;font-size:12px;}}
  .sheet-body .order-block + .order-block{{margin-top:5px;padding-top:3px;}}
  .sheet-body .card{{margin:0 auto 3mm;}}
  .sheet-body .label{{margin:0 auto 2mm;min-height:24mm;}}
"#
            .to_string()
        } else {
            String::new()
        };
        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
  @page{{size:A4 portrait;margin:10mm 12mm;}}
  *{{box-sizing:border-box;}}
  body{{font-family:"Microsoft YaHei","SimSun",sans-serif;margin:0;color:#000;font-size:13px;line-height:1.55;}}
  .sheet{{page-break-after:always;padding:0;}}
  .sheet:last-child{{page-break-after:auto;}}
  .sheet-body{{width:100%;}}
  /* 一页最多 3 单：从上往下纵向堆叠，每单满宽；块头标订单号，中间用虚线分隔 */
  .order-block{{width:100%;break-inside:avoid;page-break-inside:avoid;}}
  .order-block + .order-block{{margin-top:10px;padding-top:8px;border-top:2px dashed #999;}}
  .order-band{{background:#eef3f8;border-left:4px solid #1a6fb5;padding:4px 10px;margin-bottom:6px;font-size:13px;font-weight:900;letter-spacing:1px;}}
  .order-body{{width:100%;}}
{compact}  .batch-page-info{{display:flex;justify-content:space-between;align-items:baseline;gap:14px;font-size:11px;color:#333;border-bottom:1px solid #999;padding:0 2px 6px;margin-bottom:12px;}}
  .batch-page-info .pi-page{{white-space:nowrap;color:#555;}}
  .batch-page-info .pi-doc{{font-size:13px;font-weight:900;}}
  .batch-page-info .pi-order{{font-weight:700;}}
  .batch-page-info .pi-company{{color:#555;}}
  .head{{display:flex;align-items:center;gap:8px;margin-bottom:8px;}}
  .logo{{width:30px;height:30px;border-radius:50%;background:#1a6fb5;color:#fff;text-align:center;line-height:30px;font-size:14px;font-weight:700;flex:0 0 auto;}}
  h1{{flex:1;text-align:center;font-size:22px;font-weight:900;margin:0;letter-spacing:2px;}}
  .head .dt{{font-size:10px;color:#555;text-align:right;}}
  .page-info{{text-align:right;font-size:10px;color:#666;margin-bottom:4px;}}
  .block{{border:1px solid #000;padding:9px 10px;margin-bottom:12px;break-inside:avoid;}}
  .two-col{{display:flex;gap:0;}}
  .left{{flex:1.3;}}
  .right{{flex:1;border-left:1px solid #000;padding-left:6px;}}
  table{{width:100%;border-collapse:collapse;}}
  th,td{{border:1px solid #000;padding:6px 8px;vertical-align:middle;text-align:left;}}
  th{{background:#f4f4f4;font-weight:700;text-align:center;}}
  td.k{{width:52px;font-weight:700;background:#f4f4f4;white-space:nowrap;}}
  td.red{{color:#c00;font-weight:700;}}
  td.c{{text-align:center;}}
  td.r{{text-align:right;}}
  td.hole{{min-width:120px;font-size:11px;line-height:1.5;word-break:break-word;background:#fbfdff;font-weight:700;}}
  td.empty{{text-align:center;color:#999;}}
  .prod{{font-weight:900;color:#c00;font-size:14px;}}
  .m-title{{font-weight:900;font-size:14px;margin:8px 0 4px;}}
  .gline{{display:flex;margin:2px 0;}}
  .gline .gl{{width:56px;font-weight:700;}}
  .gline .gv{{font-weight:700;}}
  .screw{{margin-top:8px;border-top:1px dashed #999;padding-top:6px;}}
  .screw .st{{font-weight:900;margin-bottom:3px;}}
  .screw .sl{{line-height:1.7;}}
  .screw b{{color:#c00;font-size:14px;}}
  .total{{margin-top:10px;font-weight:700;font-size:14px;text-align:right;}}
  .customer-bar{{display:flex;justify-content:space-between;align-items:center;background:#fdf6d8;border:1px solid #e8d98a;padding:5px 8px;margin:8px 0 4px;font-size:13px;}}
  .customer-bar b{{font-weight:700;font-size:14px;}}
  .subtotal{{background:#fdf6d8;font-weight:700;}}
  .total-bar{{background:#f6c9d8;border:1px solid #e39bb4;text-align:center;font-size:15px;font-weight:900;padding:8px;margin-top:8px;}}
  .sign{{margin-top:22px;display:flex;justify-content:space-between;font-size:12px;}}
  .note{{margin-top:8px;font-size:11px;color:#555;line-height:1.6;}}
  .sub{{text-align:center;font-size:11px;color:#555;margin-bottom:14px;}}
  .info{{display:flex;flex-wrap:wrap;gap:6px 22px;margin-bottom:12px;font-size:14px;line-height:1.7;}}
  .info b{{font-weight:700;}}
  .notice{{margin-top:14px;font-size:12px;color:#333;line-height:1.8;border-top:1px solid #000;padding-top:8px;}}
  .card{{border:2px solid #000;margin:0 auto 10mm;width:100%;max-width:92mm;}}
  .card td.k{{width:24%;text-align:center;background:#f4f4f4;}}
  .title{{text-align:center;font-size:20px;font-weight:900;letter-spacing:6px;padding:9px 0;border-bottom:1.5px solid #000;}}
  td.v{{text-align:left;}}
  .qr{{text-align:center;font-size:10px;color:#888;width:56px;}}
  .qr-box{{border:1px dashed #999;margin:0 auto;width:48px;height:48px;line-height:48px;}}
  .label{{position:relative;border:1.5px solid #000;padding:8px 10px;margin:0 auto 4mm;width:100%;max-width:80mm;min-height:36mm;}}
  .lt{{line-height:1.9;}}
  .label .qr{{position:absolute;top:8px;right:8px;width:40px;height:40px;border:1px dashed #999;text-align:center;line-height:40px;font-size:9px;color:#999;}}
  .price-detail{{font-size:11px;color:#444;}}
  .pay-box{{display:flex;justify-content:flex-end;align-items:center;gap:10px;margin-top:10px;}}
  .pay-box .pay-qr{{border:1px dashed #999;width:54px;height:54px;line-height:54px;text-align:center;font-size:10px;color:#888;}}
</style></head><body>{parts}</body></html>"#,
            parts = parts.join(""),
            compact = compact_css.as_str(),
        ))
    }

    /// 渲染 HTML(排版:字段对齐、单位统一 mm、重复表头、金额右对齐)
    fn render(&self, order_id: i64, doc_type: &str, opts: &RenderOpts) -> Result<String, String> {
        let conn = self.db.lock().unwrap();
        // (order_no, customer, terminal_addr, terminal_phone, product_name, material_type, color,
        //  width, height, face_glass, back_glass, order_date, style, face_glass_w, face_glass_h,
        //  back_glass_w, back_glass_h, total_price, remark)
        let mut order_fields: (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<f64>, Option<f64>, Option<String>, Option<String>, Option<String>, Option<String>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<String>) = (
            None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        );
        if let Ok(row) = conn.query_row(
            "SELECT order_no, customer, terminal_addr, terminal_phone, product_name, material_type, color, \
             width, height, face_glass, back_glass, order_date, style, face_glass_w, face_glass_h, \
             back_glass_w, back_glass_h, total_price, remark FROM orders WHERE id = ?1",
            params![order_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<f64>>(7)?,
                    r.get::<_, Option<f64>>(8)?,
                    r.get::<_, Option<String>>(9)?,
                    r.get::<_, Option<String>>(10)?,
                    r.get::<_, Option<String>>(11)?,
                    r.get::<_, Option<String>>(12)?,
                    r.get::<_, Option<f64>>(13)?,
                    r.get::<_, Option<f64>>(14)?,
                    r.get::<_, Option<f64>>(15)?,
                    r.get::<_, Option<f64>>(16)?,
                    r.get::<_, Option<f64>>(17)?,
                    r.get::<_, Option<String>>(18)?,
                ))
            },
        ) {
            order_fields = row;
        }
        // 型材明细
        let profiles: Vec<(String, f64, i64)> = {
            let mut stmt = conn
                .prepare("SELECT profile_name, length, quantity FROM profiles WHERE order_id=?1 ORDER BY id")
                .ok();
            stmt.as_mut()
                .map(|s| {
                    s.query_map(params![order_id], |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                            r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                            r.get::<_, i64>(2)?,
                        ))
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|x| x.ok()).collect())
                    .unwrap_or_default()
                })
                .unwrap_or_default()
        };
        // 玻璃明细
        let glasses: Vec<(String, f64, f64, i64)> = {
            let mut stmt = conn
                .prepare("SELECT glass_name, width, height, quantity FROM glasses WHERE order_id=?1 ORDER BY id")
                .ok();
            stmt.as_mut()
                .map(|s| {
                    s.query_map(params![order_id], |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                            r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                            r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                            r.get::<_, i64>(3)?,
                        ))
                    })
                    .ok()
                    .map(|rows| rows.filter_map(|x| x.ok()).collect())
                    .unwrap_or_default()
                })
                .unwrap_or_default()
        };
        drop(conn);

        let (order_no, customer, terminal_addr, terminal_phone, product_name, material_type, color, width, height, face_glass, back_glass, order_date, style, face_glass_w, face_glass_h, back_glass_w, back_glass_h, total_price, remark) = order_fields;

        // 全系统统一「宽×高」：原来是 高×宽，和订单页相反，操作员一眼看错就做错门。
        let size_str = match (width, height) {
            (Some(w), Some(h)) => format!("{}×{}", fmt_len(w), fmt_len(h)),
            _ => String::new(),
        };
        // 总宽 / 总高(供下料单单独列出)
        let (width_str, height_str) = match (width, height) {
            (Some(w), Some(h)) => (fmt_len(w), fmt_len(h)),
            _ => (String::new(), String::new()),
        };
        // 面玻/背玻尺寸(供下料单玻璃小表)
        let face_size = match (face_glass_w, face_glass_h) {
            (Some(w), Some(h)) => format!("{}×{}", fmt_len(w), fmt_len(h)),
            _ => String::new(),
        };
        let back_size = match (back_glass_w, back_glass_h) {
            (Some(w), Some(h)) => format!("{}×{}", fmt_len(w), fmt_len(h)),
            _ => String::new(),
        };
        let order_no_s = order_no.clone().unwrap_or_default();
        let customer_s = customer.clone().unwrap_or_default();
        let product_name_s = product_name.clone().unwrap_or_default();
        let material_type_s = material_type.clone().unwrap_or_default();
        let color_s = color.clone().unwrap_or_default();
        let face_glass_s = face_glass.clone().unwrap_or_default();
        let back_glass_s = back_glass.clone().unwrap_or_default();
        let terminal_addr_s = terminal_addr.clone().unwrap_or_default();
        let terminal_phone_s = terminal_phone.clone().unwrap_or_default();
        let order_date_s = order_date.clone().unwrap_or_default();
        let style_s = style.clone().unwrap_or_default();
        let total_price_v = total_price.unwrap_or(0.0);
        let remark_s = remark.clone().unwrap_or_default();

        // 各单据共用的补充信息(对照用户提供的 7 份打印单据样本补齐)
        let (screw_qty, screw_spec) = self.screw_rule(&product_name_s, &material_type_s, opts.screw_qty);
        let page_info = opts.page_info();
        let area_m2 = match (width, height) {
            (Some(w), Some(h)) => format!("{:.3}", w * h / 1_000_000.0),
            _ => String::new(),
        };
        // 扇数:从型材明细里名称含「光企/扇」的行的数量推断,无则 0
        let fan_count: i64 = profiles
            .iter()
            .filter(|(n, _, _)| n.contains("光企") || n.contains("扇"))
            .map(|(_, _, q)| *q)
            .sum();

        // 厂名:取自设置页「公司名称」(用户样本单据里也有厂名),统一加在标题前
        let company_name: String = {
            let c = self.db.lock().unwrap();
            c.query_row(
                "SELECT value FROM app_config WHERE key='company.name'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_default()
            .trim()
            .to_string()
        };

        let html = match doc_type {
            "cutting" => self.render_cutting(&order_no_s, &customer_s, &terminal_addr_s, &product_name_s, &material_type_s, &color_s, &style_s, &size_str, &width_str, &height_str, &face_glass_s, &back_glass_s, &face_size, &back_size, &profiles, &glasses, screw_qty, &screw_spec),
            "glass_cutting" => self.render_glass_cutting(&order_no_s, &customer_s, &terminal_addr_s, &product_name_s, &color_s, &size_str, &face_glass_s, &back_glass_s, &face_size, &back_size, &glasses, &page_info, &area_m2,
                &self.glass_hole_text(&product_name_s)),
            "certificate" => self.render_certificate(&order_no_s, &customer_s, &terminal_addr_s, &product_name_s, &material_type_s, &color_s, &style_s, &size_str, &face_glass_s, &back_glass_s, &order_date_s, &remark_s),
            "delivery" => self.render_delivery(&order_no_s, &customer_s, &terminal_addr_s, &terminal_phone_s, &product_name_s, &material_type_s, &color_s, &style_s, &size_str, &face_glass_s, &back_glass_s, &order_date_s, &page_info, fan_count),
            "sales" => self.render_sales(&order_no_s, &customer_s, &terminal_addr_s, &terminal_phone_s, &product_name_s, &material_type_s, &color_s, &style_s, &size_str, &face_glass_s, &back_glass_s, &order_date_s, total_price_v, &page_info),
            "receipt" => self.render_receipt(&order_no_s, &customer_s, &terminal_addr_s, &terminal_phone_s, &product_name_s, &color_s, &style_s, &size_str, &face_glass_s, &back_glass_s, &order_date_s, total_price_v, &area_m2),
            "label" => self.render_label(&order_no_s, &customer_s, &terminal_addr_s, &product_name_s, &color_s, &style_s, &size_str, &face_glass_s, &back_glass_s),
            _ => Err(format!("未知文档类型: {}", doc_type)),
        }?;
        let html = self.apply_logo(html);
        if company_name.is_empty() {
            Ok(html)
        } else {
            // 只替换第一个 <h1>,保持各单据原有排版不变
            Ok(html.replacen("<h1>", &format!("<h1>{} · ", company_name), 1))
        }
    }

    /// 按设置处理单据头部的图标：
    /// - 关闭图标 → 整块去掉，标题自动居中；
    /// - 开启且有自定义图片 → 换成图片（用 data URL 内嵌，打印时不需要额外读文件）；
    /// - 开启但没上传 → 保留原来的「门」字圆标。
    fn apply_logo(&self, html: String) -> String {
        let (enabled, img) = {
            let conn = self.db.lock().unwrap();
            let en = crate::db::get_config(&conn, "print.logo.enabled")
                .unwrap_or_else(|| "true".to_string())
                != "false";
            let im = crate::db::get_config(&conn, "print.logo.image").unwrap_or_default();
            (en, im)
        };
        const OLD: &str = r#"<div class="logo">门</div>"#;
        if !enabled {
            return html.replace(OLD, "");
        }
        if img.trim().is_empty() {
            return html;
        }
        // 尺寸由 CSS 的 .logo 控制（24–34px），图片一律 contain，绝不会撑大版面
        let tag = format!(
            r#"<img class="logo" alt="" src="{}" style="object-fit:contain;background:#fff;border-radius:4px;">"#,
            img.trim()
        );
        html.replace(OLD, &tag)
    }

    /// 生成「客户单据」分享页：把勾选订单按勾选的单据样板渲染成一份 HTML，
    /// 交给外部浏览器打开即可直接看单子 / 打印成 PDF / 转发分享。
    /// 分享预览不是正式打印，不走尺寸核对门禁；每单一个区块、从上往下排。
    pub fn share_html(&self, order_ids: &[i64], doc_types: &[String]) -> Result<String, String> {
        if order_ids.is_empty() || doc_types.is_empty() {
            return Err("请先选择订单和单据样板".to_string());
        }
        for dt in doc_types {
            if !DOC_TYPES.contains(&dt.as_str()) {
                return Err(format!("未知文档类型: {}", dt));
            }
        }
        let mut parts: Vec<String> = Vec::new();
        for dt in doc_types {
            for &id in order_ids {
                let full = self.render(id, dt, &RenderOpts::default())?;
                let body = extract_body(&full);
                let order_no: String = {
                    let c = self.db.lock().unwrap();
                    c.query_row("SELECT order_no FROM orders WHERE id=?1", params![id], |r| r.get::<_, String>(0))
                        .unwrap_or_default()
                };
                parts.push(format!(
                    "<section class=\"share-doc\"><div class=\"share-band\">{}　|　订单 {}</div><div class=\"share-body\">{}</div></section>",
                    doc_type_name(dt.as_str()),
                    html_escape(&order_no),
                    body
                ));
            }
        }
        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>客户单据</title>
<style>
  @page{{size:A4 portrait;margin:10mm 12mm;}}
  *{{box-sizing:border-box;}}
  body{{font-family:"Microsoft YaHei","SimSun",sans-serif;margin:0;color:#000;font-size:13px;line-height:1.55;}}
  .share-doc{{page-break-after:always;padding:8px 0 16px;}}
  .share-doc:last-child{{page-break-after:auto;}}
  .share-band{{background:#eef3f8;border-left:4px solid #1a6fb5;padding:6px 12px;margin-bottom:10px;font-size:14px;font-weight:900;}}
  .share-body{{width:100%;}}
  .head{{display:flex;align-items:center;gap:8px;margin-bottom:8px;}}
  .logo{{width:28px;height:28px;border-radius:50%;background:#1a6fb5;color:#fff;text-align:center;line-height:28px;font-size:13px;font-weight:700;}}
  h1{{flex:1;text-align:center;font-size:22px;font-weight:900;margin:0;letter-spacing:2px;}}
  .head .dt,.page-info{{font-size:11px;color:#666;text-align:right;}}
  .block{{border:1px solid #000;padding:9px 10px;margin-bottom:12px;break-inside:avoid;}}
  table{{width:100%;border-collapse:collapse;}}
  th,td{{border:1px solid #000;padding:6px 8px;text-align:left;}}
  th{{background:#f4f4f4;font-weight:700;text-align:center;}}
  td.k{{width:52px;font-weight:700;background:#f4f4f4;white-space:nowrap;}}
  td.c{{text-align:center;}} td.r{{text-align:right;}} td.red{{color:#c00;font-weight:700;}}
  td.hole{{min-width:150px;font-size:12px;line-height:1.65;word-break:break-word;background:#fbfdff;font-weight:700;}}
  .info{{display:flex;flex-wrap:wrap;gap:6px 22px;margin-bottom:12px;font-size:14px;line-height:1.7;}}
  .two-col{{display:flex;gap:0;}} .left{{flex:1.3;}} .right{{flex:1;border-left:1px solid #000;padding-left:6px;}}
  .prod{{font-weight:900;color:#c00;font-size:14px;}}
  .m-title{{font-weight:900;font-size:14px;margin:8px 0 4px;}}
  .gline{{display:flex;margin:2px 0;}} .gline .gl{{width:56px;font-weight:700;}}
  .screw{{margin-top:8px;border-top:1px dashed #999;padding-top:6px;}}
  .screw .st{{font-weight:900;margin-bottom:3px;}} .screw .sl{{line-height:1.7;}}
  .screw b{{color:#c00;font-size:14px;}}
  .total,.total-bar{{margin-top:10px;font-weight:700;font-size:14px;text-align:right;}}
  .total-bar{{background:#f6c9d8;text-align:center;font-size:15px;font-weight:900;padding:8px;}}
  .customer-bar{{display:flex;justify-content:space-between;background:#fdf6d8;border:1px solid #e8d98a;padding:5px 8px;margin:8px 0 4px;font-size:13px;}}
  .subtotal{{background:#fdf6d8;font-weight:700;}}
  .sub{{text-align:center;font-size:11px;color:#555;margin-bottom:14px;}}
  .sign{{margin-top:22px;display:flex;justify-content:space-between;font-size:12px;}}
  .note{{margin-top:8px;font-size:11px;color:#555;line-height:1.6;}}
  .notice{{margin-top:14px;font-size:12px;color:#333;line-height:1.8;border-top:1px solid #000;padding-top:8px;}}
  .card{{border:2px solid #000;margin:0 auto 10mm;width:100%;max-width:92mm;}}
  .card td.k{{width:24%;text-align:center;background:#f4f4f4;}}
  .title{{text-align:center;font-size:20px;font-weight:900;letter-spacing:6px;padding:9px 0;border-bottom:1.5px solid #000;}}
  td.v{{text-align:left;}}
  .qr{{text-align:center;font-size:10px;color:#888;width:56px;}}
  .qr-box{{border:1px dashed #999;margin:0 auto;width:48px;height:48px;line-height:48px;}}
  .label{{position:relative;border:1.5px solid #000;padding:8px 10px;margin:0 auto 4mm;width:100%;max-width:80mm;min-height:36mm;}}
  .lt{{line-height:1.9;}}
  .label .qr{{position:absolute;top:8px;right:8px;width:40px;height:40px;border:1px dashed #999;text-align:center;line-height:40px;font-size:9px;color:#999;}}
  .price-detail{{font-size:11px;color:#444;}}
  .pay-box{{display:flex;justify-content:flex-end;align-items:center;gap:10px;margin-top:10px;}}
  .pay-box .pay-qr{{border:1px dashed #999;width:54px;height:54px;line-height:54px;text-align:center;font-size:10px;color:#888;}}
</style></head><body>{parts}</body></html>"#,
            parts = parts.join("")
        ))
    }

    /// 下料单(生产单:左主信息 + 右玻璃小表 + 底部型材矩阵[名称横排/长度行/数量行])
    fn render_cutting(
        &self,
        order_no: &str, customer: &str, terminal_addr: &str, product_name: &str,
        _material_type: &str, color: &str, style: &str, size_str: &str,
        width_str: &str, height_str: &str,
        face_glass: &str, back_glass: &str, face_size: &str, back_size: &str,
        profiles: &[(String, f64, i64)], glasses: &[(String, f64, f64, i64)],
        screw_qty: i64, screw_spec: &str,
    ) -> Result<String, String> {
        let now = crate::db::now_str();

        // 型材下料矩阵:名称横排 + 长度行(红) + 数量行
        let (name_cells, len_cells, qty_cells) = if profiles.is_empty() {
            (String::new(), String::new(), String::new())
        } else {
            let names = profiles.iter().map(|(n, _, _)| format!("<td class=\"c\">{}</td>", n)).collect::<String>();
            let lens = profiles.iter().map(|(_, l, _)| format!("<td class=\"c red\">{}</td>", fmt_len(*l))).collect::<String>();
            let qtys = profiles.iter().map(|(_, _, q)| format!("<td class=\"c\">{}</td>", q)).collect::<String>();
            (names, lens, qtys)
        };
        let profile_block = if profiles.is_empty() {
            "<tr><td colspan=\"5\" class=\"empty\">无型材明细</td></tr>".to_string()
        } else {
            format!(
                r#"<tr><td class="k">名称</td>{names}</tr>
                   <tr><td class="k red">长度</td>{lens}</tr>
                   <tr><td class="k">数量</td>{qtys}</tr>"#,
                names = name_cells, lens = len_cells, qtys = qty_cells,
            )
        };

        // 玻璃小表:开向/面玻/背玻 + 玻璃算式(面-白=1) + 玻璃尺寸
        let glass_sel_rows = if glasses.is_empty() {
            format!(
                "<div class=\"gline\"><span class=\"gl\">面-{}</span><span class=\"gv\">{}</span></div>
                 <div class=\"gline\"><span class=\"gl\">背-{}</span><span class=\"gv\">{}</span></div>",
                face_glass, face_size, back_glass, back_size
            )
        } else {
            glasses.iter().map(|(n, w, h, q)| {
                format!("<div class=\"gline\"><span class=\"gl\">{}</span><span class=\"gv\">{}×{}={}</span></div>", n, h, w, q)
            }).collect::<String>()
        };

        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
  @page{{size:A4 portrait;margin:8mm 10mm;}}
  *{{box-sizing:border-box;}}
  body{{font-family:"Microsoft YaHei","SimSun",sans-serif;margin:0;color:#000;font-size:13px;}}
  .head{{display:flex;align-items:center;gap:8px;margin-bottom:6px;}}
  .logo{{width:30px;height:30px;border-radius:50%;background:#1a6fb5;color:#fff;text-align:center;line-height:30px;font-size:14px;font-weight:700;flex:0 0 auto;}}
  h1{{flex:1;text-align:center;font-size:22px;font-weight:900;margin:0;letter-spacing:2px;}}
  .head .dt{{font-size:10px;color:#555;text-align:right;}}
  .block{{border:1px solid #000;padding:9px 10px;margin-bottom:12px;break-inside:avoid;}}
  .two-col{{display:flex;gap:0;}}
  .left{{flex:1.3;}}
  .right{{flex:1;border-left:1px solid #000;padding-left:6px;}}
  table{{width:100%;border-collapse:collapse;}}
  td{{border:1px solid #000;padding:5px 7px;vertical-align:middle;}}
  td.k{{width:52px;font-weight:700;background:#f4f4f4;white-space:nowrap;}}
  td.red{{color:#c00;font-weight:700;}}
  td.c{{text-align:center;}}
  td.empty{{text-align:center;color:#999;}}
  .prod{{font-weight:900;color:#c00;font-size:14px;}}
  .m-title{{font-weight:900;font-size:14px;margin:8px 0 4px;}}
  .gline{{display:flex;margin:2px 0;}}
  .gline .gl{{width:56px;font-weight:700;}}
  .gline .gv{{font-weight:700;}}
  .screw{{margin-top:8px;border-top:1px dashed #999;padding-top:6px;}}
  .screw .st{{font-weight:900;margin-bottom:3px;}}
  .screw .sl{{line-height:1.7;}}
  .screw b{{color:#c00;font-size:14px;}}
</style></head><body>
  <div class="head">
    <div class="logo">门</div>
    <h1>下 料 单</h1>
    <div class="dt">打印时间：{now}</div>
  </div>

  <div class="block">
    <div class="two-col">
      <div class="left">
        <table>
          <tr><td class="k">产品</td><td class="prod">{product_name}　{color}</td></tr>
          <tr><td class="k">尺寸</td><td>高:{height_str}　宽:{width_str}</td></tr>
          <tr><td class="k">单号</td><td>{order_no}</td></tr>
          <tr><td class="k">客户</td><td>{customer}</td></tr>
          <tr><td class="k">地址</td><td>{terminal_addr}</td></tr>
          <tr><td class="k">备注</td><td></td></tr>
        </table>
      </div>
      <div class="right">
        <table>
          <tr><td class="k">款式</td><td>{style}</td></tr>
          <tr><td class="k">面玻</td><td>{face_glass}</td></tr>
          <tr><td class="k">背玻</td><td>{back_glass}</td></tr>
        </table>
        {glass_sel_rows}
        <div class="screw">
          <div class="st">螺丝</div>
          <div class="sl">数量：<b>{screw_qty}</b> 颗</div>
          <div class="sl">型号：{screw_spec}</div>
        </div>
      </div>
    </div>

    <div class="m-title">型材下料</div>
    <table class="matrix">{profile_block}</table>
  </div>
</body></html>"#,
            order_no = order_no, customer = customer, terminal_addr = terminal_addr,
            product_name = product_name, color = color, style = style,
            width_str = width_str, height_str = height_str,
            profile_block = profile_block, glass_sel_rows = glass_sel_rows, now = now,
            screw_qty = screw_qty, screw_spec = screw_spec,
        ))
    }

    /// 玻璃下料单(报玻璃/玻璃清单:单号/客户/地址/玻璃规格/高度/宽度 + 合计块数与面积)
    fn render_glass_cutting(
        &self,
        order_no: &str, customer: &str, terminal_addr: &str, product_name: &str,
        color: &str, size_str: &str, face_glass: &str, back_glass: &str,
        face_size: &str, back_size: &str,
        glasses: &[(String, f64, f64, i64)],
        page_info: &str, area_m2: &str, hole_text: &str,
    ) -> Result<String, String> {
        let now = crate::db::now_str();
        // 玻璃规格串:面-白 = 1;背-砂 = 1
        let glass_spec = if glasses.is_empty() {
            let mut parts = Vec::new();
            if !face_glass.is_empty() { parts.push(format!("面-{} = 1", face_glass)); }
            if !back_glass.is_empty() { parts.push(format!("背-{} = 1", back_glass)); }
            parts.join(";")
        } else {
            glasses.iter().map(|(n, _, _, q)| format!("{} = {}", n, q)).collect::<Vec<_>>().join(";")
        };
        // 合计块数
        let total_qty: i64 = if glasses.is_empty() {
            let mut t = 0;
            if !face_glass.is_empty() { t += 1; }
            if !back_glass.is_empty() { t += 1; }
            t
        } else {
            glasses.iter().map(|(_, _, _, q)| q).sum()
        };
        // 玻璃型号(具体规格名称,与「玻璃规格」简写分列,避免看错)
        let glass_model = if glasses.is_empty() {
            let mut parts = Vec::new();
            if !face_glass.is_empty() { parts.push(format!("面玻:{}", face_glass)); }
            if !back_glass.is_empty() { parts.push(format!("背玻:{}", back_glass)); }
            parts.join("<br>")
        } else {
            glasses.iter().map(|(n, _, _, _)| n.clone()).collect::<Vec<_>>().join("<br>")
        };
        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
  @page{{size:A4 portrait;margin:10mm 12mm;}}
  *{{box-sizing:border-box;}}
  body{{font-family:"Microsoft YaHei","SimSun",sans-serif;margin:0;color:#000;font-size:13px;}}
  .head{{display:flex;align-items:center;gap:8px;margin-bottom:6px;}}
  .logo{{width:30px;height:30px;border-radius:50%;background:#1a6fb5;color:#fff;text-align:center;line-height:30px;font-size:14px;font-weight:700;flex:0 0 auto;}}
  h1{{flex:1;text-align:center;font-size:22px;font-weight:900;margin:0;letter-spacing:2px;}}
  .head .dt{{font-size:10px;color:#555;text-align:right;}}
  .page-info{{text-align:right;font-size:10px;color:#666;margin-bottom:4px;}}
  table{{width:100%;border-collapse:collapse;}}
  th,td{{border:1px solid #000;padding:6px 8px;text-align:left;}}
  th{{background:#f4f4f4;font-weight:700;text-align:center;}}
  td.c{{text-align:center;}}
  td.hole{{min-width:150px;font-size:12px;line-height:1.65;word-break:break-word;background:#fbfdff;font-weight:700;}}
  .total{{margin-top:10px;font-weight:700;font-size:14px;text-align:right;}}
</style></head><body>
  <div class="head">
    <div class="logo">门</div>
    <h1>玻璃清单</h1>
    <div class="dt">打印时间：{now}</div>
  </div>
  <div class="page-info">{page_info}</div>
  <table>
    <thead><tr><th>单号</th><th>客户</th><th>地址</th><th>玻璃型号</th><th>玻璃规格</th><th>开孔（平开门）</th><th>面玻尺寸(mm)</th><th>背玻尺寸(mm)</th></tr></thead>
    <tbody>
      <tr>
        <td class="c">{order_no}</td>
        <td class="c">{customer}</td>
        <td>{terminal_addr}</td>
        <td>{glass_model}</td>
        <td>{glass_spec}</td>
        <td class="hole">{hole_text}</td>
        <td class="c">{face_size}</td>
        <td class="c">{back_size}</td>
      </tr>
    </tbody>
  </table>
  <div class="total">合计：{total_qty} 块；面积：{area_m2} ㎡</div>
</body></html>"#,
            order_no = order_no, customer = customer, terminal_addr = terminal_addr,
            glass_spec = glass_spec, face_size = face_size, back_size = back_size,
            total_qty = total_qty, now = now, glass_model = glass_model,
            page_info = page_info, area_m2 = area_m2, hole_text = hole_text,
        ))
    }

    /// 合格证(标签式:左列标签加粗/右列值,右上二维码,仿参考图)
    fn render_certificate(
        &self,
        order_no: &str, customer: &str, terminal_addr: &str, product_name: &str,
        material_type: &str, color: &str, style: &str, size_str: &str,
        face_glass: &str, back_glass: &str, order_date: &str, remark: &str,
    ) -> Result<String, String> {
        // 客户名、地址、备注、颜色这些是操作员手输的，含 < & " 时会把 HTML 结构冲坏，
        // 拼模板前统一转义（尺寸是数字，不转义）。
        let order_no = html_escape(order_no);
        let customer = html_escape(customer);
        let terminal_addr = html_escape(terminal_addr);
        let product_name = html_escape(product_name);
        let material_type = html_escape(material_type);
        let color = html_escape(color);
        let style = html_escape(style);
        let face_glass = html_escape(face_glass);
        let back_glass = html_escape(back_glass);
        let order_date = html_escape(order_date);
        let remark = html_escape(remark);
        // 玻璃字段:面玻+背玻
        let glass_str = if back_glass.is_empty() {
            face_glass.clone()
        } else {
            format!("{} + {}", face_glass, back_glass)
        };
        // 纸张与二维码位置都来自设置，默认 75×100mm、二维码留白 20mm
        let (pw, ph, qr_on, qr_mm) = {
            let conn = self.db.lock().unwrap();
            let num = |k: &str, d: f64| -> f64 {
                crate::db::get_config(&conn, k)
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .filter(|v| *v > 5.0 && *v < 300.0)
                    .unwrap_or(d)
            };
            let on = crate::db::get_config(&conn, "print.qr.enabled")
                .unwrap_or_else(|| "true".to_string())
                != "false";
            (
                num("print.cert.width_mm", 75.0),
                num("print.cert.height_mm", 100.0),
                on,
                num("print.qr.size_mm", 20.0),
            )
        };
        // 字号随纸张放大：75×100 用 15px，更大的纸按比例变大，不会「小纸溢出、大纸留白」。
        // 行高由 CSS 均分铺满整张纸，所以字越大越饱满。
        let fs = (ph / 100.0 * 15.0).clamp(9.0, 30.0);
        let tfs = (fs * 1.36).clamp(12.0, 42.0);
        let kfs = (fs * 0.96).clamp(9.0, 30.0);
        // 二维码方块：不能超过跨两行的高度，否则会把表格撑歪
        let title_h = (ph * 0.09).clamp(5.0, 14.0);
        let body_h = ph - title_h;
        // 底部「备注」栏单独放大（占表格区 30%），用于手写备注 / 盖章 / 签字；
        // 其余 10 行均分剩下的 70%，仍铺满整张纸。
        let remark_h = (body_h * 0.30).max(12.0);
        let row_h = (body_h - remark_h) / 10.0;
        let qr_box = qr_mm.min(row_h * 1.86).max(6.0);
        let qr_w = (qr_box + 3.0).min(pw * 0.40);
        // 二维码由用户自己贴/自己印，这里只留一块空白。
        // 关键：关掉二维码时整张表只有 2 列（标签 + 值），此时任何 colspan="2"
        // 都会让浏览器凭空多出一列空白，所以跨列宽度必须跟着开关走。
        let (qrcell, vspan) = if qr_on {
            (
                r#"<td class="qr" rowspan="2"><div class="qr-box"></div></td>"#.to_string(),
                r#" colspan="2""#,
            )
        } else {
            (String::new(), "")
        };
        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
  /* 合格证 · 小标签纸版
     两个关键点：
     1. body 锁死纸张尺寸，避免横向溢出把文字挤成竖排；
     2. 表格用 tablewrap + height:100% 让 11 行**均分铺满**整张纸，
        不再挤在上半截、下半截留一大片空白。 */
  @page{{size:{pw}mm {ph}mm;margin:0;}}
  *{{box-sizing:border-box;}}
  html,body{{margin:0;padding:0;}}
  body{{
    width:{pw}mm;height:{ph}mm;overflow:hidden;
    font-family:"Microsoft YaHei","SimHei",sans-serif;color:#000;font-size:{fs}px;
  }}
  .card{{width:100%;height:100%;border:1.4px solid #000;display:flex;flex-direction:column;}}
  .title{{
    flex:0 0 auto;text-align:center;font-size:{tfs}px;font-weight:900;letter-spacing:2px;
    line-height:1.15;padding:{tpad}mm 0;border-bottom:1.2px solid #000;
  }}
  .tablewrap{{flex:1 1 auto;min-height:0;}}
  table{{width:100%;height:100%;border-collapse:collapse;table-layout:fixed;}}
  td{{
    border:0.8px solid #000;padding:1px 4px;vertical-align:middle;
    font-size:{fs}px;line-height:1.22;overflow:hidden;word-break:break-all;
  }}
  td.k{{width:22%;font-weight:800;text-align:center;background:#e9e9e9;font-size:{kfs}px;}}
  td.v{{text-align:left;font-weight:700;}}
  /* 底部备注栏：明显更大，内容靠上，方便写备注/盖章 */
  td.remark{{height:{remark_h}mm;vertical-align:top;padding-top:3px;}}
  .qr{{width:{qr_w}mm;padding:1mm;vertical-align:middle;}}
  .qr-box{{width:{qr_box}mm;height:{qr_box}mm;margin:0 auto;border:0.3px dashed #c8c8c8;}}
</style></head><body>
  <div class="card">
    <div class="title">合 格 证</div>
    <div class="tablewrap">
    <table>
      <tr><td class="k">单号</td><td class="v">{order_no}</td>{qrcell}</tr>
      <tr><td class="k">客户</td><td class="v">{customer}</td></tr>
      <tr><td class="k">产品</td><td class="v"{vspan}>{product_name}</td></tr>
      <tr><td class="k">颜色</td><td class="v"{vspan}>{color}</td></tr>
      <tr><td class="k">料型</td><td class="v"{vspan}>{material_type}</td></tr>
      <tr><td class="k">开向</td><td class="v"{vspan}>{style}</td></tr>
      <tr><td class="k">尺寸</td><td class="v"{vspan}>{size_str}</td></tr>
      <tr><td class="k">玻璃</td><td class="v"{vspan}>{glass_str}</td></tr>
      <tr><td class="k">地址</td><td class="v"{vspan}>{terminal_addr}</td></tr>
      <tr><td class="k">日期</td><td class="v"{vspan}>{order_date}</td></tr>
      <tr><td class="k">备注</td><td class="v remark"{vspan}>{remark}</td></tr>
    </table>
    </div>
  </div>
</body></html>"#,
            order_no = order_no, customer = customer, terminal_addr = terminal_addr,
            product_name = product_name, material_type = material_type, color = color,
            style = style, size_str = size_str, glass_str = glass_str, order_date = order_date,
            pw = pw, ph = ph, qrcell = qrcell, vspan = vspan, remark = remark,
            fs = fs, tfs = tfs, kfs = kfs, qr_w = qr_w, qr_box = qr_box, remark_h = remark_h,
            tpad = (ph / 100.0 * 1.6).clamp(0.6, 4.0),
        ))
    }

    /// 发货清单(按客户分组 + 小计 + 合计,仿门窗厂发货清单排版)
    fn render_delivery(
        &self,
        order_no: &str, customer: &str, terminal_addr: &str, terminal_phone: &str,
        product_name: &str, material_type: &str, color: &str, style: &str,
        size_str: &str, face_glass: &str, back_glass: &str, order_date: &str,
        page_info: &str, fan_count: i64,
    ) -> Result<String, String> {
        let now = crate::db::now_str();
        let material_suffix = if material_type.is_empty() {
            String::new()
        } else {
            let mut s = format!("　({})", material_type);
            if !style.is_empty() { s.push_str(&format!("/{}", style)); }
            s
        };
        let glass_suffix = if face_glass.is_empty() && back_glass.is_empty() {
            String::new()
        } else if back_glass.is_empty() {
            format!("　{}", face_glass)
        } else {
            format!("　{}+{}", face_glass, back_glass)
        };
        // 面积(mm² -> m²),保留3位
        let area = if size_str.contains('×') {
            let parts: Vec<&str> = size_str.split('×').collect();
            if parts.len() == 2 {
                if let (Ok(w), Ok(h)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
                    format!("{:.3}", w * h / 1_000_000.0)
                } else { String::new() }
            } else { String::new() }
        } else { String::new() };
        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
  @page{{size:A4 portrait;margin:10mm 12mm;}}
  *{{box-sizing:border-box;}}
  body{{font-family:"Microsoft YaHei","SimSun",sans-serif;margin:0;color:#000;font-size:12px;}}
  .head{{display:flex;align-items:center;gap:10px;margin-bottom:2px;}}
  .logo{{width:34px;height:34px;border-radius:50%;background:#1a6fb5;color:#fff;text-align:center;line-height:34px;font-size:15px;font-weight:700;flex:0 0 auto;}}
  h1{{flex:1;text-align:center;font-size:22px;font-weight:900;margin:0;letter-spacing:2px;}}
  .head .dt{{font-size:10px;color:#555;text-align:right;}}
  .page-info{{text-align:right;font-size:10px;color:#666;margin-bottom:4px;}}
  .customer-bar{{display:flex;justify-content:space-between;align-items:center;background:#fdf6d8;border:1px solid #e8d98a;padding:5px 8px;margin:8px 0 4px;font-size:13px;}}
  .customer-bar b{{font-weight:700;font-size:14px;}}
  .subtotal{{background:#fdf6d8;font-weight:700;}}
  table{{width:100%;border-collapse:collapse;}}
  th,td{{border:1px solid #000;padding:5px 6px;text-align:left;}}
  th{{background:#fdf6d8;font-weight:700;text-align:center;}}
  td.c{{text-align:center;}}
  td.r{{text-align:right;}}
  .total-bar{{background:#f6c9d8;border:1px solid #e39bb4;text-align:center;font-size:15px;font-weight:900;padding:8px;margin-top:8px;}}
  .sign{{margin-top:22px;display:flex;justify-content:space-between;font-size:12px;}}
</style></head><body>
  <div class="head">
    <div class="logo">门</div>
    <h1>发货清单</h1>
    <div class="dt">打印时间：{now}</div>
  </div>
  <div class="page-info">{page_info}</div>

  <div class="customer-bar">
    <span><b>{customer}</b>　电话：{terminal_phone}　地址：{terminal_addr}　物流：__________</span>
    <span class="subtotal">小计：1 单；{fan_count} 扇；{area}㎡</span>
  </div>
  <table>
    <thead><tr>
      <th>订单号</th><th>产品名称</th><th>颜色</th><th>尺寸</th><th>备注</th><th>终端地址</th><th>下单日期</th>
    </tr></thead>
    <tbody>
      <tr>
        <td class="c">{order_no}</td>
        <td>{product_name}{material_suffix}</td>
        <td class="c">{color}</td>
        <td class="c">{size_str}{glass_suffix}</td>
        <td></td>
        <td>{terminal_addr}</td>
        <td class="c">{order_date}</td>
      </tr>
    </tbody>
  </table>

  <div class="total-bar">合计：1 单；{fan_count} 扇；{area}㎡</div>

  <div class="sign"><span>发货人：__________</span><span>收货人：__________</span><span>日期：__________</span></div>
</body></html>"#,
            order_no = order_no, customer = customer, terminal_addr = terminal_addr,
            terminal_phone = terminal_phone, product_name = product_name, material_suffix = material_suffix,
            color = color, size_str = size_str, glass_suffix = glass_suffix,
            order_date = order_date, now = now, area = area,
            page_info = page_info, fan_count = fan_count,
        ))
    }

    /// 销售单(按客户分组,含单价/面积/金额/价格详细,仿参考图销售单)
    fn render_sales(
        &self,
        order_no: &str, customer: &str, terminal_addr: &str, terminal_phone: &str,
        product_name: &str, _material_type: &str, color: &str, style: &str,
        size_str: &str, face_glass: &str, back_glass: &str, order_date: &str,
        total_price: f64, page_info: &str,
    ) -> Result<String, String> {
        let now = crate::db::now_str();
        // 面积(从 size_str "宽×高" 计算 m²)
        let area = if size_str.contains('×') {
            let parts: Vec<&str> = size_str.split('×').collect();
            if parts.len() == 2 {
                if let (Ok(w), Ok(h)) = (parts[0].trim().parse::<f64>(), parts[1].trim().parse::<f64>()) {
                    format!("{:.3}", w * h / 1_000_000.0)
                } else { String::new() }
            } else { String::new() }
        } else { String::new() };
        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
  @page{{size:A4 portrait;margin:10mm 12mm;}}
  *{{box-sizing:border-box;}}
  body{{font-family:"Microsoft YaHei","SimSun",sans-serif;margin:0;color:#000;font-size:12px;}}
  .head{{display:flex;align-items:center;gap:8px;margin-bottom:6px;}}
  .logo{{width:30px;height:30px;border-radius:50%;background:#1a6fb5;color:#fff;text-align:center;line-height:30px;font-size:14px;font-weight:700;flex:0 0 auto;}}
  h1{{flex:1;text-align:center;font-size:22px;font-weight:900;margin:0;letter-spacing:2px;}}
  .head .dt{{font-size:10px;color:#555;text-align:right;}}
  .page-info{{text-align:right;font-size:10px;color:#666;margin-bottom:4px;}}
  .customer-bar{{display:flex;justify-content:space-between;background:#fdf6d8;border:1px solid #e8d98a;padding:5px 8px;margin:8px 0 4px;}}
  .customer-bar b{{font-weight:700;}}
  .subtotal{{background:#fdf6d8;border:1px solid #e8d98a;padding:4px 8px;margin:4px 0;font-weight:700;}}
  table{{width:100%;border-collapse:collapse;}}
  th,td{{border:1px solid #000;padding:5px 6px;text-align:left;}}
  th{{background:#fdf6d8;font-weight:700;text-align:center;}}
  td.c{{text-align:center;}}
  td.r{{text-align:right;}}
  .total-bar{{background:#f6c9d8;border:1px solid #e39bb4;text-align:right;font-size:14px;font-weight:900;padding:7px;margin-top:8px;}}
  .note{{margin-top:8px;font-size:11px;color:#555;line-height:1.6;}}
</style></head><body>
  <div class="head">
    <div class="logo">门</div>
    <h1>销 售 单</h1>
    <div class="dt">打印时间：{now}</div>
  </div>
  <div class="page-info">{page_info}</div>
  <div class="customer-bar">
    <span><b>客户：{customer}</b>　电话：{terminal_phone}　地址：{terminal_addr}</span>
    <span>下单日期：{order_date}　物流：__________</span>
  </div>
  <div class="subtotal">小计：数量 1；金额 ¥{total_price}　已付 ¥{total_price}　尾款 ¥0.00</div>
  <table>
    <thead><tr><th>单号</th><th>产品名称</th><th>颜色</th><th>尺寸(宽×高)</th><th>款式</th><th class="r">单价</th><th class="r">面积(㎡)</th><th class="r">金额</th><th>价格详细</th><th>备注</th></tr></thead>
    <tbody>
      <tr>
        <td class="c">{order_no}</td>
        <td>{product_name}</td>
        <td class="c">{color}</td>
        <td class="c">{size_str}</td>
        <td class="c">{style}</td>
        <td class="r">{total_price}</td>
        <td class="r">{area}</td>
        <td class="r">{total_price}</td>
        <td>基础价:{total_price}×1={total_price}</td>
        <td></td>
      </tr>
    </tbody>
  </table>
  <div class="total-bar">合计：数量 1；已付 ¥{total_price}　总金额 ¥{total_price}　本次尾款 ¥0.00</div>
  <div class="note">型材的颜色、厚度、质量均以看到的实物为准。<br>因洞口不标准的原因造成的问题，商家概不负责。</div>
</body></html>"#,
            order_no = order_no, customer = customer, terminal_addr = terminal_addr,
            terminal_phone = terminal_phone, product_name = product_name, color = color,
            style = style, size_str = size_str, order_date = order_date,
            total_price = total_price, area = area, now = now, page_info = page_info,
        ))
    }

    /// 回执单(客户回执:客户信息 + 订单明细表 + 合计,仿参考图回执单)
    fn render_receipt(
        &self,
        order_no: &str, customer: &str, terminal_addr: &str, terminal_phone: &str,
        product_name: &str, color: &str, style: &str, size_str: &str,
        face_glass: &str, back_glass: &str, order_date: &str, total_price: f64,
        area_m2: &str,
    ) -> Result<String, String> {
        let now = crate::db::now_str();
        let glass_str = if back_glass.is_empty() {
            face_glass.to_string()
        } else {
            format!("{}+{}", face_glass, back_glass)
        };
        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
  @page{{size:A4 portrait;margin:12mm 14mm;}}
  *{{box-sizing:border-box;}}
  body{{font-family:"Microsoft YaHei","SimSun",sans-serif;margin:0;color:#000;font-size:13px;}}
  h1{{text-align:center;font-size:22px;font-weight:900;letter-spacing:6px;margin:0 0 4px;}}
  .sub{{text-align:center;font-size:11px;color:#555;margin-bottom:14px;}}
  .info{{display:flex;flex-wrap:wrap;gap:6px 22px;margin-bottom:12px;font-size:14px;line-height:1.7;}}
  .info b{{font-weight:700;}}
  .subtotal{{background:#fdf6d8;border:1px solid #e8d98a;padding:4px 8px;margin-bottom:6px;font-weight:700;}}
  table{{width:100%;border-collapse:collapse;}}
  th,td{{border:1px solid #000;padding:5px 7px;text-align:left;}}
  th{{background:#f4f4f4;font-weight:700;text-align:center;}}
  td.c{{text-align:center;}}
  td.r{{text-align:right;}}
  .total{{margin-top:8px;text-align:right;font-weight:700;font-size:14px;}}
  .notice{{margin-top:14px;font-size:12px;color:#333;line-height:1.8;border-top:1px solid #000;padding-top:8px;}}
  .sign{{margin-top:30px;display:flex;justify-content:space-between;font-size:13px;}}
  .price-detail{{font-size:11px;color:#444;}}
  .pay-box{{display:flex;justify-content:flex-end;align-items:center;gap:10px;margin-top:10px;font-size:12px;}}
  .pay-qr{{border:1px dashed #999;width:54px;height:54px;line-height:54px;text-align:center;font-size:10px;color:#888;}}
</style></head><body>
  <h1>客 户 回 执 单</h1>
  <div class="sub">订单号：{order_no}　打印时间：{now}　指定物流：__________</div>
  <div class="info">
    <span><b>客户名称：</b>{customer}</span>
    <span><b>客户电话：</b>{terminal_phone}</span>
    <span><b>客户地址：</b>{terminal_addr}</span>
    <span><b>下单日期：</b>[ {order_date} ]</span>
  </div>
  <div class="subtotal">小计：1 单；{total_price}元　已付 {total_price}元；尾款 0.00元</div>
  <table>
    <thead><tr><th>单号</th><th>产品名称</th><th>颜色</th><th>尺寸</th><th>玻璃</th><th>开向</th><th class="r">单价</th><th class="r">面积(㎡)</th><th class="r">金额</th><th>备注</th></tr></thead>
    <tbody>
      <tr>
        <td class="c">{order_no}</td>
        <td>{product_name}</td>
        <td class="c">{color}</td>
        <td class="c">{size_str}</td>
        <td class="c">{glass_str}</td>
        <td class="c">{style}</td>
        <td class="r">{total_price}</td>
        <td class="r">{area_m2}</td>
        <td class="r">{total_price}</td>
        <td></td>
      </tr>
      <tr class="price-detail"><td colspan="10">价格详细：基础价 {total_price} × 1 = {total_price}</td></tr>
    </tbody>
  </table>
  <div class="total">合计：1 单；金额：{total_price}元；已付：{total_price}元；尾款：0元</div>
  <div class="pay-box"><span>扫码付款</span><div class="pay-qr">二维码</div></div>
  <div class="notice">客户须知：<br>1. 请当场核对产品数量、规格、颜色及玻璃，确认无误后签字。<br>2. 如有质量问题，请在收货后 7 日内联系我们处理。</div>
  <div class="sign"><span>发货人签字：__________</span><span>收货人签字：__________</span><span>日期：__________</span></div>
</body></html>"#,
            order_no = order_no, customer = customer, terminal_addr = terminal_addr,
            terminal_phone = terminal_phone, product_name = product_name, color = color,
            style = style, size_str = size_str, glass_str = glass_str,
            order_date = order_date, total_price = total_price, now = now,
            area_m2 = area_m2,
        ))
    }

    /// 标签(小标签,一张纸排多张,含二维码占位)
    fn render_label(
        &self,
        order_no: &str, customer: &str, terminal_addr: &str, product_name: &str,
        color: &str, style: &str, size_str: &str, face_glass: &str, back_glass: &str,
    ) -> Result<String, String> {
        let glass_str = if back_glass.is_empty() {
            face_glass.to_string()
        } else {
            format!("{}+{}", face_glass, back_glass)
        };
        // 一张标签,纵向重复两次模拟多联(每张之间留间隙)
        let one_label = format!(
            r#"<div class="label">
      <div class="lt">单号：{order_no}</div>
      <div class="lt">客户：{customer}</div>
      <div class="lt">产品：{product_name}</div>
      <div class="lt">颜色：{color}　款式：{style}</div>
      <div class="lt">尺寸：{size_str}</div>
      <div class="lt">玻璃：{glass_str}</div>
      <div class="lt">地址：{terminal_addr}</div>
      <div class="lt">备注：__________</div>
      <div class="qr">二维码</div>
    </div>"#,
            order_no = order_no, customer = customer, terminal_addr = terminal_addr,
            product_name = product_name,
            color = color, style = style, size_str = size_str, glass_str = glass_str,
        );
        Ok(format!(
            r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
  @page{{size:A4 portrait;margin:8mm 10mm;}}
  *{{box-sizing:border-box;}}
  body{{font-family:"Microsoft YaHei","SimSun",sans-serif;margin:0;color:#000;font-size:12px;}}
  .label{{position:relative;border:1.5px solid #000;padding:8px 10px;margin:0 auto 4mm;width:100%;max-width:80mm;min-height:36mm;}}
  .lt{{line-height:1.9;}}
  .qr{{position:absolute;top:8px;right:8px;width:40px;height:40px;border:1px dashed #999;text-align:center;line-height:40px;font-size:9px;color:#999;}}
</style></head><body>
  {labels}
</body></html>"#,
            labels = format!("{}{}", one_label, one_label),
        ))
    }
}

/// 格式化长度:整数不带小数,非整数保留1位
fn fmt_len(v: f64) -> String {
    if (v - v.round()).abs() < 1e-6 {
        format!("{}", v.round() as i64)
    } else {
        format!("{:.1}", v)
    }
}

/// 提取 HTML 中 <body>...</body> 之间的内容(用于拼接多单据)
fn extract_body(html: &str) -> String {
    let lower = html.to_lowercase();
    let start = match lower.find("<body") {
        Some(i) => {
            // 找到 body 标签的结束 >
            match html[i..].find('>') {
                Some(j) => i + j + 1,
                None => return html.to_string(),
            }
        }
        None => return html.to_string(),
    };
    let end = match lower.rfind("</body>") {
        Some(i) => i,
        None => html.len(),
    };
    html[start..end].to_string()
}
