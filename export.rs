// src-tauri/src/export.rs
// 订单导出(Excel xlsx / CSV),自主实现,Apache 2.0
// 导出前需尺寸已核对(由调用方保证),导出后订单状态置为「已审核」

use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};

/// 导出结果:文件路径 + 文件名
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportResult {
    pub path: String,
    pub filename: String,
}

/// 软件内表格预览的数据结构：与导出的 xlsx 逐格同源
#[derive(Debug, Clone, serde::Serialize)]
pub struct SheetPreview {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

const SUMMARY_COLS: [&str; 24] = [
    "订单号", "合同号", "客户", "终端地址", "终端电话", "业务员", "产品名称", "料型",
    "颜色", "款式", "尺寸宽(mm)", "尺寸高(mm)", "玻璃层数", "面玻", "背玻",
    "面玻宽(mm)", "面玻高(mm)", "背玻宽(mm)", "背玻高(mm)", "订单总价(元)",
    "状态", "下单日期", "交货日期", "备注",
];
const DETAILS: [(&str, &str, &[&str]); 3] = [
    ("profiles", "SELECT o.order_no,p.profile_name,p.category,p.length,p.quantity FROM profiles p JOIN orders o ON o.id=p.order_id WHERE p.order_id IN (", &["订单号", "型材名称", "分类", "长度(mm)", "数量"]),
    ("glasses", "SELECT o.order_no,g.glass_name,g.width,g.height,g.quantity FROM glasses g JOIN orders o ON o.id=g.order_id WHERE g.order_id IN (", &["订单号", "玻璃名称", "宽(mm)", "高(mm)", "数量"]),
    ("accessories", "SELECT o.order_no,a.acc_name,a.quantity,a.remark FROM accessories a JOIN orders o ON o.id=a.order_id WHERE a.order_id IN (", &["订单号", "配件名称", "数量", "备注"]),
];

/// 订单号是用户手填的，直接拼进文件名会造成路径穿越或写文件失败。
/// 过滤 Windows 非法字符、前后导点/空格，并限制长度。
pub fn safe_filename_part(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == ' ');
    let out: String = trimmed.chars().take(80).collect();
    if out.is_empty() {
        "order".to_string()
    } else {
        out
    }
}

pub struct ExportService {
    pub db: Arc<Mutex<Connection>>,
}

impl ExportService {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        ExportService { db }
    }

    /// 导出单个订单为 xlsx 或 csv
    pub fn export_order(&self, order_id: i64, format: &str) -> Result<ExportResult, String> {
        if format != "xlsx" && format != "csv" {
            return Err(format!("不支持的导出格式: {}", format));
        }
        let conn = self.db.lock().unwrap();
        let order = self.load_order(&conn, order_id)?;
        let profiles = self.load_profiles(&conn, order_id)?;
        let glasses = self.load_glasses(&conn, order_id)?;
        drop(conn);

        let dir = crate::db::data_dir().join("export");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let filename = format!(
            "订单_{}_{}.{}",
            safe_filename_part(&order.order_no),
            chrono::Local::now().format("%Y%m%d_%H%M%S%3f"),
            format
        );
        let path = dir.join(&filename);

        match format {
            "xlsx" => self.write_xlsx(&path, &order, &profiles, &glasses)?,
            "csv" => self.write_csv(&path, &order, &profiles, &glasses)?,
            _ => return Err(format!("不支持的导出格式: {}", format)),
        }

        Ok(ExportResult {
            path: path.to_string_lossy().to_string(),
            filename,
        })
    }

    /// 收集表格数据。导出 xlsx 与软件内预览共用此方法，保证两者逐格一致。
    fn collect_sheets(&self, order_ids: &[i64], sections: &[String]) -> Result<Vec<SheetPreview>, String> {
        if order_ids.is_empty() {
            return Err("请先勾选要导出的订单".to_string());
        }
        let ph = order_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let conn = self.db.lock().unwrap();
        let mut out: Vec<SheetPreview> = Vec::new();

        let sql = format!(
            "SELECT order_no,contract_no,customer,terminal_addr,terminal_phone,salesman,product_name,\
             material_type,color,style,width,height,glass_layer,face_glass,back_glass,face_glass_w,\
             face_glass_h,back_glass_w,back_glass_h,total_price,status,order_date,delivery_date,remark \
             FROM orders WHERE id IN ({}) ORDER BY id",
            ph
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(order_ids.iter()), |r| {
                Ok((0..SUMMARY_COLS.len())
                    .map(|i| r.get::<_, Option<String>>(i).ok().flatten().unwrap_or_default())
                    .collect::<Vec<String>>())
            })
            .map_err(|e| e.to_string())?;
        out.push(SheetPreview {
            name: "订单汇总".to_string(),
            headers: SUMMARY_COLS.iter().map(|s| s.to_string()).collect(),
            rows: rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?,
        });

        for (key, sql_head, headers) in DETAILS {
            if !sections.iter().any(|s| s == key) {
                continue;
            }
            let q = format!("{}{}) ORDER BY o.id", sql_head, ph);
            let mut st = conn.prepare(&q).map_err(|e| e.to_string())?;
            let rr = st
                .query_map(rusqlite::params_from_iter(order_ids.iter()), |r| {
                    Ok((0..headers.len())
                        .map(|i| r.get::<_, Option<String>>(i).ok().flatten().unwrap_or_default())
                        .collect::<Vec<String>>())
                })
                .map_err(|e| e.to_string())?;
            out.push(SheetPreview {
                name: match key { "profiles" => "型材明细", "glasses" => "玻璃明细", _ => "配件明细" }.to_string(),
                headers: headers.iter().map(|s| s.to_string()).collect(),
                rows: rr.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?,
            });
        }
        Ok(out)
    }

    /// 软件内直接查看表格（不生成文件也能看）
    pub fn preview_orders_batch(&self, order_ids: &[i64], sections: &[String]) -> Result<Vec<SheetPreview>, String> {
        self.collect_sheets(order_ids, sections)
    }

    /// 订单管理页「一键导出 Excel」：多订单汇总成一张表，按勾选追加明细 sheet。
    /// sections 可含 profiles / glasses / accessories；汇总表始终导出。
    pub fn export_orders_batch(
        &self,
        order_ids: &[i64],
        sections: &[String],
    ) -> Result<ExportResult, String> {
        use rust_xlsxwriter::*;
        let sheets = self.collect_sheets(order_ids, sections)?;
        let dir = crate::db::data_dir().join("export");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let filename = format!("订单汇总_{}.xlsx", chrono::Local::now().format("%Y%m%d_%H%M%S%3f"));
        let path = dir.join(&filename);

        let mut wb = Workbook::new();
        let fmt_header = Format::new().set_bold().set_background_color(Color::RGB(0xD9E2F3));
        for sp in &sheets {
            let ws = wb.add_worksheet();
            ws.set_name(&sp.name).map_err(|e| e.to_string())?;
            for (c, h) in sp.headers.iter().enumerate() {
                ws.write_string_with_format(0, c as u16, h.as_str(), &fmt_header)
                    .map_err(|e| e.to_string())?;
            }
            for (i, row) in sp.rows.iter().enumerate() {
                for (c, v) in row.iter().enumerate() {
                    ws.write_string((i + 1) as u32, c as u16, v.as_str())
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        wb.save(&path).map_err(|e| e.to_string())?;
        Ok(ExportResult {
            path: path.to_string_lossy().to_string(),
            filename,
        })
    }

    fn load_order(&self, conn: &Connection, order_id: i64) -> Result<OrderExport, String> {
        conn.query_row(
            "SELECT order_no, contract_no, customer, terminal_addr, terminal_phone, \
             product_name, material_type, color, width, height, face_glass, back_glass, \
             glass_layer, face_glass_w, face_glass_h, back_glass_w, back_glass_h, \
             style, total_price, status, order_date, delivery_date, remark \
             FROM orders WHERE id = ?1",
            params![order_id],
            |r| {
                Ok(OrderExport {
                    order_no: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    contract_no: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    customer: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    terminal_addr: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    terminal_phone: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    product_name: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    material_type: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    color: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    width: r.get::<_, Option<f64>>(8)?,
                    height: r.get::<_, Option<f64>>(9)?,
                    face_glass: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                    back_glass: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
                    glass_layer: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
                    face_glass_w: r.get::<_, Option<f64>>(13)?,
                    face_glass_h: r.get::<_, Option<f64>>(14)?,
                    back_glass_w: r.get::<_, Option<f64>>(15)?,
                    back_glass_h: r.get::<_, Option<f64>>(16)?,
                    style: r.get::<_, Option<String>>(17)?.unwrap_or_default(),
                    total_price: r.get::<_, Option<f64>>(18)?.unwrap_or(0.0),
                    status: r.get::<_, Option<String>>(19)?.unwrap_or_default(),
                    order_date: r.get::<_, Option<String>>(20)?.unwrap_or_default(),
                    delivery_date: r.get::<_, Option<String>>(21)?.unwrap_or_default(),
                    remark: r.get::<_, Option<String>>(22)?.unwrap_or_default(),
                })
            },
        )
        .map_err(|e| e.to_string())
    }

    fn load_profiles(&self, conn: &Connection, order_id: i64) -> Result<Vec<(String, String, f64, i64)>, String> {
        let mut stmt = conn
            .prepare("SELECT profile_name, category, length, quantity FROM profiles WHERE order_id=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![order_id], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    fn load_glasses(&self, conn: &Connection, order_id: i64) -> Result<Vec<(String, f64, f64, i64)>, String> {
        let mut stmt = conn
            .prepare("SELECT glass_name, width, height, quantity FROM glasses WHERE order_id=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![order_id], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                    r.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    fn write_xlsx(
        &self,
        path: &std::path::Path,
        order: &OrderExport,
        profiles: &[(String, String, f64, i64)],
        glasses: &[(String, f64, f64, i64)],
    ) -> Result<(), String> {
        use rust_xlsxwriter::*;
        let mut wb = Workbook::new();
        let fmt_bold = Format::new().set_bold();
        let fmt_header = Format::new().set_bold().set_background_color(Color::RGB(0xD9E2F3));

        // Sheet1: 订单信息
        let ws = wb.add_worksheet();
        ws.set_name("订单信息").map_err(|e| e.to_string())?;
        let rows: Vec<(&str, String)> = vec![
            ("订单号", order.order_no.clone()),
            ("合同号", order.contract_no.clone()),
            ("客户", order.customer.clone()),
            ("终端地址", order.terminal_addr.clone()),
            ("终端电话", order.terminal_phone.clone()),
            ("产品名称", order.product_name.clone()),
            ("料型", order.material_type.clone()),
            ("颜色", order.color.clone()),
            ("款式", order.style.clone()),
            ("尺寸宽(mm)", order.width.map(|v| v.to_string()).unwrap_or_default()),
            ("尺寸高(mm)", order.height.map(|v| v.to_string()).unwrap_or_default()),
            ("玻璃层数", order.glass_layer.clone()),
            ("面玻", order.face_glass.clone()),
            ("背玻", order.back_glass.clone()),
            ("面玻尺寸宽(mm)", order.face_glass_w.map(|v| v.to_string()).unwrap_or_default()),
            ("面玻尺寸高(mm)", order.face_glass_h.map(|v| v.to_string()).unwrap_or_default()),
            ("背玻尺寸宽(mm)", order.back_glass_w.map(|v| v.to_string()).unwrap_or_default()),
            ("背玻尺寸高(mm)", order.back_glass_h.map(|v| v.to_string()).unwrap_or_default()),
            ("订单总价(元)", order.total_price.to_string()),
            ("状态", order.status.clone()),
            ("下单日期", order.order_date.clone()),
            ("交货日期", order.delivery_date.clone()),
            ("备注", order.remark.clone()),
        ];
        for (i, (k, v)) in rows.iter().enumerate() {
            let r = i as u32;
            ws.write_string_with_format(r, 0, *k, &fmt_bold).map_err(|e| e.to_string())?;
            ws.write_string(r, 1, v.as_str()).map_err(|e| e.to_string())?;
        }

        // Sheet2: 型材明细
        let ws2 = wb.add_worksheet();
        ws2.set_name("型材明细").map_err(|e| e.to_string())?;
        let headers = ["型材名称", "类型", "长度(mm)", "数量"];
        for (c, h) in headers.iter().enumerate() {
            ws2.write_string_with_format(0, c as u16, *h, &fmt_header).map_err(|e| e.to_string())?;
        }
        for (i, (n, cat, l, q)) in profiles.iter().enumerate() {
            let r = (i + 1) as u32;
            ws2.write_string(r, 0, n.as_str()).map_err(|e| e.to_string())?;
            ws2.write_string(r, 1, cat.as_str()).map_err(|e| e.to_string())?;
            ws2.write_number(r, 2, *l).map_err(|e| e.to_string())?;
            ws2.write_number(r, 3, *q as f64).map_err(|e| e.to_string())?;
        }

        // Sheet3: 玻璃明细
        let ws3 = wb.add_worksheet();
        ws3.set_name("玻璃明细").map_err(|e| e.to_string())?;
        let headers3 = ["玻璃名称", "宽(mm)", "高(mm)", "数量"];
        for (c, h) in headers3.iter().enumerate() {
            ws3.write_string_with_format(0, c as u16, *h, &fmt_header).map_err(|e| e.to_string())?;
        }
        for (i, (n, w, h, q)) in glasses.iter().enumerate() {
            let r = (i + 1) as u32;
            ws3.write_string(r, 0, n.as_str()).map_err(|e| e.to_string())?;
            ws3.write_number(r, 1, *w).map_err(|e| e.to_string())?;
            ws3.write_number(r, 2, *h).map_err(|e| e.to_string())?;
            ws3.write_number(r, 3, *q as f64).map_err(|e| e.to_string())?;
        }

        wb.save(path).map_err(|e| e.to_string())
    }

    fn write_csv(
        &self,
        path: &std::path::Path,
        order: &OrderExport,
        profiles: &[(String, String, f64, i64)],
        glasses: &[(String, f64, f64, i64)],
    ) -> Result<(), String> {
        let mut out = String::new();
        // BOM(Excel 识别 UTF-8)
        out.push('\u{feff}');
        out.push_str("订单号,合同号,客户,终端地址,产品名称,料型,颜色,款式,尺寸宽(mm),尺寸高(mm),玻璃层数,面玻,背玻,面玻宽(mm),面玻高(mm),背玻宽(mm),背玻高(mm),总价(元),状态,下单日期,交货日期,备注\n");
        let row = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            csv_escape(&order.order_no),
            csv_escape(&order.contract_no),
            csv_escape(&order.customer),
            csv_escape(&order.terminal_addr),
            csv_escape(&order.product_name),
            csv_escape(&order.material_type),
            csv_escape(&order.color),
            csv_escape(&order.style),
            order.width.map(|v| v.to_string()).unwrap_or_default(),
            order.height.map(|v| v.to_string()).unwrap_or_default(),
            csv_escape(&order.glass_layer),
            csv_escape(&order.face_glass),
            csv_escape(&order.back_glass),
            order.face_glass_w.map(|v| v.to_string()).unwrap_or_default(),
            order.face_glass_h.map(|v| v.to_string()).unwrap_or_default(),
            order.back_glass_w.map(|v| v.to_string()).unwrap_or_default(),
            order.back_glass_h.map(|v| v.to_string()).unwrap_or_default(),
            order.total_price,
            csv_escape(&order.status),
            csv_escape(&order.order_date),
            csv_escape(&order.delivery_date),
            csv_escape(&order.remark),
        );
        out.push_str(&row);

        out.push_str("\n型材明细\n");
        out.push_str("型材名称,类型,长度(mm),数量\n");
        for (n, cat, l, q) in profiles {
            out.push_str(&format!("{},{},{},{}\n", csv_escape(n), csv_escape(cat), l, q));
        }

        out.push_str("\n玻璃明细\n");
        out.push_str("玻璃名称,宽(mm),高(mm),数量\n");
        for (n, w, h, q) in glasses {
            out.push_str(&format!("{},{},{},{}\n", csv_escape(n), w, h, q));
        }

        std::fs::write(path, out).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Default)]
struct OrderExport {
    order_no: String,
    contract_no: String,
    customer: String,
    terminal_addr: String,
    terminal_phone: String,
    product_name: String,
    material_type: String,
    color: String,
    width: Option<f64>,
    height: Option<f64>,
    face_glass: String,
    back_glass: String,
    glass_layer: String,
    face_glass_w: Option<f64>,
    face_glass_h: Option<f64>,
    back_glass_w: Option<f64>,
    back_glass_h: Option<f64>,
    style: String,
    total_price: f64,
    status: String,
    order_date: String,
    delivery_date: String,
    remark: String,
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
