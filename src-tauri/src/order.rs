// src-tauri/src/order.rs
// 订单 + 客户 + 合同 + 型材/玻璃/配件 + 生产环节(父子层级构件表 + 递归查询)
// 合规说明:构件关系采用关系型父子层级表(parent_id / path / 递归 CTE),不用无向图/邻接表。

use crate::model::{
    Accessory, Component, Contract, ContractInput, Customer, CustomerInput, Glass, GlassLib,
    GlassType, Order, OrderInput, ProductPreset, ProductionStep, Profile, ProfileLib,
};
use crate::plugin::PluginHost;
use rusqlite::{params, Connection, Row};
use std::sync::{Arc, Mutex};

const ORDER_COLS: &str = "id, order_no, contract_no, customer, customer_id, terminal_addr, \
     terminal_phone, material_type, product_name, color, width, height, face_glass, back_glass, \
     glass_layer, face_glass_w, face_glass_h, back_glass_w, back_glass_h, \
     style, remark, total_price, status, mark, contract_status, order_date, delivery_date, \
     inbound_date, outbound_date, salesman, operator, created_at, updated_at, \
     fan_count, track_type, door_height, wall_thickness, unit_price";

fn row_to_order(row: &Row) -> rusqlite::Result<Order> {
    Ok(Order {
        id: row.get(0)?,
        order_no: row.get(1)?,
        contract_no: row.get(2)?,
        customer: row.get(3)?,
        customer_id: row.get(4)?,
        terminal_addr: row.get(5)?,
        terminal_phone: row.get(6)?,
        material_type: row.get(7)?,
        product_name: row.get(8)?,
        color: row.get(9)?,
        width: row.get(10)?,
        height: row.get(11)?,
        face_glass: row.get(12)?,
        back_glass: row.get(13)?,
        glass_layer: row.get(14)?,
        face_glass_w: row.get(15)?,
        face_glass_h: row.get(16)?,
        back_glass_w: row.get(17)?,
        back_glass_h: row.get(18)?,
        style: row.get(19)?,
        remark: row.get(20)?,
        total_price: row.get(21)?,
        status: row.get(22)?,
        mark: row.get(23)?,
        contract_status: row.get(24)?,
        order_date: row.get(25)?,
        delivery_date: row.get(26)?,
        inbound_date: row.get(27)?,
        outbound_date: row.get(28)?,
        salesman: row.get(29)?,
        operator: row.get(30)?,
        created_at: row.get(31)?,
        updated_at: row.get(32)?,
        fan_count: row.get(33)?,
        track_type: row.get(34)?,
        door_height: row.get(35)?,
        wall_thickness: row.get(36)?,
        unit_price: row.get(37)?,
        step_summary: None,
    })
}

pub struct OrderService {
    pub db: Arc<Mutex<Connection>>,
    pub plugin_host: std::sync::Arc<PluginHost>,
}

impl OrderService {
    pub fn new(db: Arc<Mutex<Connection>>, plugin_host: std::sync::Arc<PluginHost>) -> Self {
        OrderService { db, plugin_host }
    }

    fn fire_order_saved(&self, order: &Order) {
        let payload = serde_json::json!({
            "api_version": "1.0",
            "hook": "order.saved",
            "payload": { "orderId": order.id, "order": order, "changedFields": ["save"] }
        });
        let _ = self.plugin_host.dispatch("order.saved", &payload);
    }

    // ---------- 订单 ----------

    pub fn create(&self, input: OrderInput) -> Result<Order, String> {
        // 先留一份客户信息快照：下面 INSERT 会把 input 的字段 move 掉
        let cust_name = input.customer.clone();
        let cust_phone = input.terminal_phone.clone();
        let cust_addr = input.terminal_addr.clone();
        let cust_id_in = input.customer_id;
        let conn = self.db.lock().unwrap();
        conn.execute(
            "INSERT INTO orders(order_no, contract_no, customer, customer_id, terminal_addr, \
             terminal_phone, material_type, product_name, color, width, height, face_glass, \
             back_glass, glass_layer, face_glass_w, face_glass_h, back_glass_w, back_glass_h, \
             style, remark, total_price, status, mark, contract_status, order_date, \
             delivery_date, salesman, operator, fan_count, track_type, door_height, wall_thickness, unit_price) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33)",
            params![
                input.order_no,
                input.contract_no,
                input.customer,
                input.customer_id,
                input.terminal_addr,
                input.terminal_phone,
                input.material_type,
                input.product_name,
                input.color,
                input.width,
                input.height,
                input.face_glass,
                input.back_glass,
                input.glass_layer,
                input.face_glass_w,
                input.face_glass_h,
                input.back_glass_w,
                input.back_glass_h,
                input.style,
                input.remark,
                input.total_price.unwrap_or(0.0),
                input.status.unwrap_or_else(|| "未合并".to_string()),
                input.mark,
                input.contract_status,
                input.order_date,
                input.delivery_date,
                input.salesman,
                input.operator,
                input.fan_count,
                input.track_type,
                input.door_height,
                input.wall_thickness,
                input.unit_price,
            ],
        )
        .map_err(|e| e.to_string())?;
        let id = conn.last_insert_rowid();
        // 自动保存客户：订单里填了客户名就进客户表，并回填 customer_id
        if let Some((cid, _)) = upsert_customer(&conn, cust_name.as_deref(), cust_phone.as_deref(), cust_addr.as_deref()) {
            if cust_id_in.is_none() {
                conn.execute("UPDATE orders SET customer_id=?1 WHERE id=?2", params![cid, id]).ok();
            }
        }
        drop(conn);
        let order = self.get(id)?;
        self.fire_order_saved(&order);
        self.save_history(&[
            ("customer", order.customer.clone()),
            ("terminal_addr", order.terminal_addr.clone()),
            ("terminal_phone", order.terminal_phone.clone()),
            ("salesman", order.salesman.clone()),
        ]);
        Ok(order)
    }

    pub fn get(&self, id: i64) -> Result<Order, String> {
        let conn = self.db.lock().unwrap();
        conn.query_row(
            &format!("SELECT {} FROM orders WHERE id = ?1", ORDER_COLS),
            params![id],
            row_to_order,
        )
        .map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Result<Vec<Order>, String> {
        let conn = self.db.lock().unwrap();
        let mut orders: Vec<Order> = {
            let mut stmt = conn
                .prepare(&format!("SELECT {} FROM orders ORDER BY id DESC", ORDER_COLS))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], row_to_order)
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };

        // 为「生产中」订单填充环节进度汇总
        for o in orders.iter_mut() {
            if o.status == "生产中" {
                o.step_summary = Some(self.step_summary_inner(&conn, o.id));
            }
        }
        drop(conn);
        Ok(orders)
    }

    /// 汇总某订单的生产环节进度(需持有 conn 锁)
    fn step_summary_inner(&self, conn: &Connection, order_id: i64) -> String {
        let mut done: Vec<String> = Vec::new();
        let mut undone: Vec<String> = Vec::new();
        if let Ok(mut stmt) = conn.prepare("SELECT step_name, done FROM production_steps WHERE order_id=?1 ORDER BY id") {
            if let Ok(rows) = stmt.query_map(params![order_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            }) {
                for r in rows.flatten() {
                    if r.1 == 1 { done.push(r.0); } else { undone.push(r.0); }
                }
            }
        }
        match (done.is_empty(), undone.is_empty()) {
            (true, true) => "未开始".to_string(),
            (true, false) => format!("未完成:{}", undone.join("/")),
            (false, true) => format!("已完成:{}", done.join("/")),
            (false, false) => format!("完成:{} | 未完:{}", done.join("/"), undone.join("/")),
        }
    }

    pub fn update(&self, id: i64, input: OrderInput) -> Result<Order, String> {
        let cur = self.get(id)?;
        let conn = self.db.lock().unwrap();
        conn.execute(
            "UPDATE orders SET order_no=?1, contract_no=?2, customer=?3, customer_id=?4, \
             terminal_addr=?5, terminal_phone=?6, material_type=?7, product_name=?8, color=?9, \
             width=?10, height=?11, face_glass=?12, back_glass=?13, glass_layer=?14, \
             face_glass_w=?15, face_glass_h=?16, back_glass_w=?17, back_glass_h=?18, \
             style=?19, remark=?20, \
             total_price=?21, status=?22, mark=?23, contract_status=?24, order_date=?25, \
             delivery_date=?26, inbound_date=?27, outbound_date=?28, salesman=?29, operator=?30, \
             fan_count=?31, track_type=?32, door_height=?33, wall_thickness=?34, unit_price=?35, \
             updated_at=datetime('now','localtime') WHERE id=?36",
            params![
                input.order_no,
                input.contract_no.or(cur.contract_no),
                input.customer.or(cur.customer),
                input.customer_id.or(cur.customer_id),
                input.terminal_addr.or(cur.terminal_addr),
                input.terminal_phone.or(cur.terminal_phone),
                input.material_type.or(cur.material_type),
                input.product_name.or(cur.product_name),
                input.color.or(cur.color),
                input.width.or(cur.width),
                input.height.or(cur.height),
                input.face_glass.or(cur.face_glass),
                input.back_glass.or(cur.back_glass),
                input.glass_layer.or(cur.glass_layer),
                input.face_glass_w.or(cur.face_glass_w),
                input.face_glass_h.or(cur.face_glass_h),
                input.back_glass_w.or(cur.back_glass_w),
                input.back_glass_h.or(cur.back_glass_h),
                input.style.or(cur.style),
                input.remark.or(cur.remark),
                input.total_price.unwrap_or(cur.total_price),
                input.status.unwrap_or(cur.status),
                input.mark.or(cur.mark),
                input.contract_status.or(cur.contract_status),
                input.order_date.or(cur.order_date),
                input.delivery_date.or(cur.delivery_date),
                input.inbound_date.or(cur.inbound_date),
                input.outbound_date.or(cur.outbound_date),
                input.salesman.or(cur.salesman),
                input.operator.or(cur.operator),
                input.fan_count.or(cur.fan_count),
                input.track_type.or(cur.track_type),
                input.door_height.or(cur.door_height),
                input.wall_thickness.or(cur.wall_thickness),
                input.unit_price.or(cur.unit_price),
                id,
            ],
        )
        .map_err(|e| e.to_string())?;
        // 自动保存客户：取更新后的实际值写入客户表（同名只补空字段）
        if let Ok((cn, cp, ca)) = conn.query_row(
            "SELECT customer, terminal_phone, terminal_addr FROM orders WHERE id=?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        ) {
            upsert_customer(&conn, cn.as_deref(), cp.as_deref(), ca.as_deref());
        }
        drop(conn);
        let order = self.get(id)?;
        self.fire_order_saved(&order);
        self.save_history(&[
            ("customer", order.customer.clone()),
            ("terminal_addr", order.terminal_addr.clone()),
            ("terminal_phone", order.terminal_phone.clone()),
            ("salesman", order.salesman.clone()),
        ]);
        Ok(order)
    }

    /// 更新订单状态(用于生产流转)
    pub fn set_status(&self, id: i64, status: &str) -> Result<Order, String> {
        let conn = self.db.lock().unwrap();
        conn.execute(
            "UPDATE orders SET status=?1, updated_at=datetime('now','localtime') WHERE id=?2",
            params![status, id],
        )
        .map_err(|e| e.to_string())?;
        drop(conn);
        self.get(id)
    }

    /// 只清空订单下的型材/玻璃/配件明细（编辑订单时先清后写，保留订单本身）
    pub fn clear_details(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM profiles WHERE order_id = ?1", params![id]).ok();
        conn.execute("DELETE FROM glasses WHERE order_id = ?1", params![id]).ok();
        conn.execute("DELETE FROM accessories WHERE order_id = ?1", params![id]).ok();
        Ok(())
    }

    pub fn remove(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM components WHERE order_id = ?1", params![id]).ok();
        conn.execute("DELETE FROM profiles WHERE order_id = ?1", params![id]).ok();
        conn.execute("DELETE FROM glasses WHERE order_id = ?1", params![id]).ok();
        conn.execute("DELETE FROM accessories WHERE order_id = ?1", params![id]).ok();
        conn.execute("DELETE FROM production_steps WHERE order_id = ?1", params![id]).ok();
        conn.execute("DELETE FROM orders WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ---------- 客户 ----------

    pub fn customer_list(&self) -> Result<Vec<Customer>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, phone, address, terminal_addr, contact, remark, brand, created_at FROM customers ORDER BY id DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Customer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    phone: row.get(2)?,
                    address: row.get(3)?,
                    terminal_addr: row.get(4)?,
                    contact: row.get(5)?,
                    remark: row.get(6)?,
                    brand: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn customer_save(&self, input: CustomerInput) -> Result<Customer, String> {
        let conn = self.db.lock().unwrap();
        match input.id {
            Some(id) => {
                conn.execute(
                    "UPDATE customers SET name=?1, phone=?2, address=?3, terminal_addr=?4, contact=?5, remark=?6, brand=?7 WHERE id=?8",
                    params![input.name, input.phone, input.address, input.terminal_addr, input.contact, input.remark, input.brand, id],
                ).map_err(|e| e.to_string())?;
                drop(conn);
                self.customer_get(id)
            }
            None => {
                conn.execute(
                    "INSERT INTO customers(name, phone, address, terminal_addr, contact, remark, brand) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![input.name, input.phone, input.address, input.terminal_addr, input.contact, input.remark, input.brand],
                ).map_err(|e| e.to_string())?;
                let id = conn.last_insert_rowid();
                drop(conn);
                self.customer_get(id)
            }
        }
    }

    pub fn customer_get(&self, id: i64) -> Result<Customer, String> {
        let conn = self.db.lock().unwrap();
        conn.query_row(
            "SELECT id, name, phone, address, terminal_addr, contact, remark, brand, created_at FROM customers WHERE id=?1",
            params![id],
            |row| {
                Ok(Customer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    phone: row.get(2)?,
                    address: row.get(3)?,
                    terminal_addr: row.get(4)?,
                    contact: row.get(5)?,
                    remark: row.get(6)?,
                    brand: row.get(7)?,
                    created_at: row.get(8)?,
                })
            },
        )
        .map_err(|e| e.to_string())
    }

    pub fn customer_remove(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM customers WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ---------- 历史输入候选(客户/地址/电话/业务员,拼音匹配在前端做) ----------

    /// 保存一批历史候选(幂等去重)。下单/改单后把客户、终端地址、终端电话、业务员各自存一份。
    pub fn save_history(&self, entries: &[(&str, Option<String>)]) {
        let conn = self.db.lock().unwrap();
        for (key, val) in entries {
            if let Some(v) = val {
                let v = v.trim();
                if v.is_empty() {
                    continue;
                }
                conn.execute(
                    "INSERT INTO history(field_key, value) VALUES(?1, ?2) \
                     ON CONFLICT(field_key, value) DO UPDATE SET created_at=datetime('now','localtime')",
                    params![key, v],
                )
                .ok();
            }
        }
    }

    /// 按字段取历史候选,按最近使用排序(最新在前)
    pub fn history_list(&self, field_key: &str) -> Result<Vec<String>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT value FROM history WHERE field_key=?1 \
                 ORDER BY created_at DESC, rowid DESC LIMIT 200",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![field_key], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// 某型号系列实际支持的轨道类型(从其公式条件里「轨道类型=X」去重而来)
    pub fn series_tracks(&self, series: &str) -> Result<Vec<String>, String> {
        let mut v: Vec<String> = {
            let conn = self.db.lock().unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT REPLACE(condition,'轨道类型=','') FROM profile_lib \
                     WHERE series=?1 AND condition LIKE '轨道类型=%' AND TRIM(REPLACE(condition,'轨道类型=',''))<>''",
                )
                .map_err(|e| e.to_string())?;
            let mut pv: Vec<String> = stmt
                .query_map(params![series], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .filter_map(|x| x.ok())
                .collect();
            // 玻璃表里也可能带轨道条件,一并纳入
            let mut stmt2 = conn
                .prepare(
                    "SELECT DISTINCT REPLACE(condition,'轨道类型=','') FROM glass_lib \
                     WHERE series=?1 AND condition LIKE '轨道类型=%' AND TRIM(REPLACE(condition,'轨道类型=',''))<>''",
                )
                .map_err(|e| e.to_string())?;
            let mut gv: Vec<String> = stmt2
                .query_map(params![series], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .filter_map(|x| x.ok())
                .collect();
            pv.append(&mut gv);
            pv
        };
        // 去重,并按常见顺序排序(地轨/吊轨/自粘下轨/串轨 在前)
        let order = ["地轨", "吊轨", "自粘下轨", "串轨"];
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for t in order {
            if v.contains(&t.to_string()) && seen.insert(t.to_string()) {
                out.push(t.to_string());
            }
        }
        for t in v {
            if seen.insert(t.clone()) {
                out.push(t);
            }
        }
        Ok(out)
    }

    // ---------- 合同 ----------

    pub fn contract_list(&self) -> Result<Vec<Contract>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, contract_no, contract_date, customer, terminal_addr, total_price, paid_amount, erase_zero, balance, order_count, pay_progress, inbound_progress, outbound_progress, product_name, logistics, logistics_no, mark, remark, created_at, updated_at FROM contracts ORDER BY id DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Contract {
                    id: row.get(0)?,
                    contract_no: row.get(1)?,
                    contract_date: row.get(2)?,
                    customer: row.get(3)?,
                    terminal_addr: row.get(4)?,
                    total_price: row.get(5)?,
                    paid_amount: row.get(6)?,
                    erase_zero: row.get(7)?,
                    balance: row.get(8)?,
                    order_count: row.get(9)?,
                    pay_progress: row.get(10)?,
                    inbound_progress: row.get(11)?,
                    outbound_progress: row.get(12)?,
                    product_name: row.get(13)?,
                    logistics: row.get(14)?,
                    logistics_no: row.get(15)?,
                    mark: row.get(16)?,
                    remark: row.get(17)?,
                    created_at: row.get(18)?,
                    updated_at: row.get(19)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn contract_get(&self, id: i64) -> Result<Contract, String> {
        let conn = self.db.lock().unwrap();
        conn.query_row(
            "SELECT id, contract_no, contract_date, customer, terminal_addr, total_price, paid_amount, erase_zero, balance, order_count, pay_progress, inbound_progress, outbound_progress, product_name, logistics, logistics_no, mark, remark, created_at, updated_at FROM contracts WHERE id=?1",
            params![id],
            |row| {
                Ok(Contract {
                    id: row.get(0)?,
                    contract_no: row.get(1)?,
                    contract_date: row.get(2)?,
                    customer: row.get(3)?,
                    terminal_addr: row.get(4)?,
                    total_price: row.get(5)?,
                    paid_amount: row.get(6)?,
                    erase_zero: row.get(7)?,
                    balance: row.get(8)?,
                    order_count: row.get(9)?,
                    pay_progress: row.get(10)?,
                    inbound_progress: row.get(11)?,
                    outbound_progress: row.get(12)?,
                    product_name: row.get(13)?,
                    logistics: row.get(14)?,
                    logistics_no: row.get(15)?,
                    mark: row.get(16)?,
                    remark: row.get(17)?,
                    created_at: row.get(18)?,
                    updated_at: row.get(19)?,
                })
            },
        )
        .map_err(|e| e.to_string())
    }

    pub fn contract_save(&self, input: ContractInput) -> Result<Contract, String> {
        let conn = self.db.lock().unwrap();
        match input.id {
            Some(id) => {
                conn.execute(
                    "UPDATE contracts SET contract_no=?1, contract_date=?2, customer=?3, terminal_addr=?4, total_price=?5, paid_amount=?6, erase_zero=?7, balance=?8, order_count=?9, pay_progress=?10, inbound_progress=?11, outbound_progress=?12, product_name=?13, logistics=?14, logistics_no=?15, mark=?16, remark=?17, updated_at=datetime('now','localtime') WHERE id=?18",
                    params![
                        input.contract_no, input.contract_date, input.customer, input.terminal_addr,
                        input.total_price.unwrap_or(0.0), input.paid_amount.unwrap_or(0.0),
                        input.erase_zero.unwrap_or(0.0), input.balance.unwrap_or(0.0),
                        input.order_count.unwrap_or(0), input.pay_progress, input.inbound_progress, input.outbound_progress,
                        input.product_name, input.logistics, input.logistics_no, input.mark, input.remark, id
                    ],
                ).map_err(|e| e.to_string())?;
                drop(conn);
                self.contract_get(id)
            }
            None => {
                conn.execute(
                    "INSERT INTO contracts(contract_no, contract_date, customer, terminal_addr, total_price, paid_amount, erase_zero, balance, order_count, pay_progress, inbound_progress, outbound_progress, product_name, logistics, logistics_no, mark, remark) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                    params![
                        input.contract_no, input.contract_date, input.customer, input.terminal_addr,
                        input.total_price.unwrap_or(0.0), input.paid_amount.unwrap_or(0.0),
                        input.erase_zero.unwrap_or(0.0), input.balance.unwrap_or(0.0),
                        input.order_count.unwrap_or(0), input.pay_progress, input.inbound_progress, input.outbound_progress,
                        input.product_name, input.logistics, input.logistics_no, input.mark, input.remark
                    ],
                ).map_err(|e| e.to_string())?;
                let id = conn.last_insert_rowid();
                drop(conn);
                self.contract_get(id)
            }
        }
    }

    pub fn contract_remove(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM contracts WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ---------- 构件 ----------

    pub fn add_component(&self, comp: Component) -> Result<Component, String> {
        let conn = self.db.lock().unwrap();
        let path = match comp.parent_id {
            Some(pid) => {
                let parent_path: Option<String> = conn
                    .query_row("SELECT path FROM components WHERE id = ?1", params![pid], |r| r.get(0))
                    .ok()
                    .flatten();
                match parent_path {
                    Some(pp) => format!("{}{}/", pp, pid),
                    None => "/".to_string(),
                }
            }
            None => "/".to_string(),
        };
        conn.execute(
            "INSERT INTO components(order_id, parent_id, path, name, spec, material_type, \
             orig_width, orig_height, actual_width, actual_height, quantity, sort_order) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                comp.order_id, comp.parent_id, path, comp.name, comp.spec, comp.material_type,
                comp.orig_width, comp.orig_height, comp.actual_width, comp.actual_height,
                comp.quantity, comp.sort_order,
            ],
        )
        .map_err(|e| e.to_string())?;
        let id = conn.last_insert_rowid();
        drop(conn);
        let c = self.get_component(id)?;
        if let Ok(order) = self.get(comp.order_id) {
            self.fire_order_saved(&order);
        }
        Ok(c)
    }

    pub fn get_component(&self, id: i64) -> Result<Component, String> {
        let conn = self.db.lock().unwrap();
        conn.query_row(
            "SELECT id, order_id, parent_id, path, name, spec, material_type, \
             orig_width, orig_height, actual_width, actual_height, quantity, sort_order \
             FROM components WHERE id = ?1",
            params![id],
            |row| {
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
            },
        )
        .map_err(|e| e.to_string())
    }

    pub fn list_components(&self, order_id: i64) -> Result<Vec<Component>, String> {
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
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn update_component(&self, id: i64, comp: Component) -> Result<Component, String> {
        let conn = self.db.lock().unwrap();
        conn.execute(
            "UPDATE components SET name=?1, spec=?2, material_type=?3, \
             orig_width=?4, orig_height=?5, actual_width=?6, actual_height=?7, \
             quantity=?8, sort_order=?9, parent_id=?10 WHERE id=?11",
            params![
                comp.name, comp.spec, comp.material_type, comp.orig_width, comp.orig_height,
                comp.actual_width, comp.actual_height, comp.quantity, comp.sort_order,
                comp.parent_id, id,
            ],
        )
        .map_err(|e| e.to_string())?;
        drop(conn);
        self.get_component(id)
    }

    pub fn remove_component(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM components WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn descendants(&self, root_id: i64) -> Result<Vec<Component>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "WITH RECURSIVE tree(id) AS ( \
                   SELECT id FROM components WHERE id = ?1 \
                   UNION ALL \
                   SELECT c.id FROM components c JOIN tree t ON c.parent_id = t.id \
                 ) \
                 SELECT c.id, c.order_id, c.parent_id, c.path, c.name, c.spec, c.material_type, \
                   c.orig_width, c.orig_height, c.actual_width, c.actual_height, c.quantity, c.sort_order \
                 FROM components c WHERE c.id IN (SELECT id FROM tree) ORDER BY c.sort_order",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![root_id], |row| {
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
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    // ---------- 型材 ----------

    pub fn profile_list(&self, order_id: i64) -> Result<Vec<Profile>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, order_id, profile_name, length, quantity, category, remark FROM profiles WHERE order_id=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![order_id], |row| {
                Ok(Profile {
                    id: row.get(0)?,
                    order_id: row.get(1)?,
                    profile_name: row.get(2)?,
                    length: row.get(3)?,
                    quantity: row.get(4)?,
                    category: row.get(5)?,
                    remark: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn profile_save(&self, p: Profile) -> Result<Profile, String> {
        let conn = self.db.lock().unwrap();
        if p.id > 0 {
            conn.execute(
                "UPDATE profiles SET profile_name=?1, length=?2, quantity=?3, category=?4, remark=?5 WHERE id=?6",
                params![p.profile_name, p.length, p.quantity, p.category, p.remark, p.id],
            ).map_err(|e| e.to_string())?;
            drop(conn);
            let list = self.profile_list(p.order_id)?;
            Ok(list.into_iter().find(|x| x.id == p.id).unwrap_or(p))
        } else {
            conn.execute(
                "INSERT INTO profiles(order_id, profile_name, length, quantity, category, remark) VALUES(?1,?2,?3,?4,?5,?6)",
                params![p.order_id, p.profile_name, p.length, p.quantity, p.category, p.remark],
            ).map_err(|e| e.to_string())?;
            let id = conn.last_insert_rowid();
            drop(conn);
            let list = self.profile_list(p.order_id)?;
            Ok(list.into_iter().find(|x| x.id == id).unwrap_or(Profile { id, ..p }))
        }
    }

    pub fn profile_remove(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM profiles WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    // ---------- 玻璃 ----------

    pub fn glass_list(&self, order_id: i64) -> Result<Vec<Glass>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, order_id, glass_name, width, height, quantity, remark FROM glasses WHERE order_id=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![order_id], |row| {
                Ok(Glass {
                    id: row.get(0)?,
                    order_id: row.get(1)?,
                    glass_name: row.get(2)?,
                    width: row.get(3)?,
                    height: row.get(4)?,
                    quantity: row.get(5)?,
                    remark: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn glass_save(&self, g: Glass) -> Result<Glass, String> {
        let conn = self.db.lock().unwrap();
        if g.id > 0 {
            conn.execute(
                "UPDATE glasses SET glass_name=?1, width=?2, height=?3, quantity=?4, remark=?5 WHERE id=?6",
                params![g.glass_name, g.width, g.height, g.quantity, g.remark, g.id],
            ).map_err(|e| e.to_string())?;
            drop(conn);
            let list = self.glass_list(g.order_id)?;
            Ok(list.into_iter().find(|x| x.id == g.id).unwrap_or(g))
        } else {
            conn.execute(
                "INSERT INTO glasses(order_id, glass_name, width, height, quantity, remark) VALUES(?1,?2,?3,?4,?5,?6)",
                params![g.order_id, g.glass_name, g.width, g.height, g.quantity, g.remark],
            ).map_err(|e| e.to_string())?;
            let id = conn.last_insert_rowid();
            drop(conn);
            let list = self.glass_list(g.order_id)?;
            Ok(list.into_iter().find(|x| x.id == id).unwrap_or(Glass { id, ..g }))
        }
    }

    pub fn glass_remove(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM glasses WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    // ---------- 配件 ----------

    pub fn accessory_list(&self, order_id: i64) -> Result<Vec<Accessory>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, order_id, acc_name, quantity, remark FROM accessories WHERE order_id=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![order_id], |row| {
                Ok(Accessory {
                    id: row.get(0)?,
                    order_id: row.get(1)?,
                    acc_name: row.get(2)?,
                    quantity: row.get(3)?,
                    remark: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn accessory_save(&self, a: Accessory) -> Result<Accessory, String> {
        let conn = self.db.lock().unwrap();
        if a.id > 0 {
            conn.execute(
                "UPDATE accessories SET acc_name=?1, quantity=?2, remark=?3 WHERE id=?4",
                params![a.acc_name, a.quantity, a.remark, a.id],
            ).map_err(|e| e.to_string())?;
            drop(conn);
            let list = self.accessory_list(a.order_id)?;
            Ok(list.into_iter().find(|x| x.id == a.id).unwrap_or(a))
        } else {
            conn.execute(
                "INSERT INTO accessories(order_id, acc_name, quantity, remark) VALUES(?1,?2,?3,?4)",
                params![a.order_id, a.acc_name, a.quantity, a.remark],
            ).map_err(|e| e.to_string())?;
            let id = conn.last_insert_rowid();
            drop(conn);
            let list = self.accessory_list(a.order_id)?;
            Ok(list.into_iter().find(|x| x.id == id).unwrap_or(Accessory { id, ..a }))
        }
    }

    pub fn accessory_remove(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM accessories WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    // ---------- 产品预设 ----------

    pub fn preset_list(&self) -> Result<Vec<ProductPreset>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, category, material_type, style, color, face_glass, back_glass, remark, created_at FROM product_presets ORDER BY category, id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ProductPreset {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    category: row.get(2)?,
                    material_type: row.get(3)?,
                    style: row.get(4)?,
                    color: row.get(5)?,
                    face_glass: row.get(6)?,
                    back_glass: row.get(7)?,
                    remark: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn preset_save(&self, p: ProductPreset) -> Result<ProductPreset, String> {
        let conn = self.db.lock().unwrap();
        if p.id > 0 {
            conn.execute(
                "UPDATE product_presets SET name=?1, category=?2, material_type=?3, style=?4, color=?5, face_glass=?6, back_glass=?7, remark=?8 WHERE id=?9",
                params![p.name, p.category, p.material_type, p.style, p.color, p.face_glass, p.back_glass, p.remark, p.id],
            ).map_err(|e| e.to_string())?;
            drop(conn);
            let list = self.preset_list()?;
            Ok(list.into_iter().find(|x| x.id == p.id).unwrap_or(p))
        } else {
            conn.execute(
                "INSERT INTO product_presets(name, category, material_type, style, color, face_glass, back_glass, remark) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![p.name, p.category, p.material_type, p.style, p.color, p.face_glass, p.back_glass, p.remark],
            ).map_err(|e| e.to_string())?;
            let id = conn.last_insert_rowid();
            drop(conn);
            let list = self.preset_list()?;
            Ok(list.into_iter().find(|x| x.id == id).unwrap_or(ProductPreset { id, ..p }))
        }
    }

    pub fn preset_remove(&self, id: i64) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute("DELETE FROM product_presets WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
        Ok(())
    }

    // ---------- 玻璃类型清单 ----------

    pub fn glass_type_list(&self) -> Result<Vec<GlassType>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, category, name FROM glass_types ORDER BY category, sort_order, id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(GlassType {
                    id: row.get(0)?,
                    category: row.get(1)?,
                    name: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    // ---------- 产品类型→型材库 ----------

    /// 列出指定产品类型的型材库(含公式)
    pub fn profile_lib_list(&self, product_type: &str, series: &str) -> Result<Vec<ProfileLib>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, product_type, series, profile_name, formula, quantity, category, sort_order, condition, quantity_formula \
                 FROM profile_lib WHERE product_type=?1 AND series=?2 ORDER BY sort_order, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![product_type, series], |row| {
                Ok(ProfileLib {
                    id: row.get(0)?,
                    product_type: row.get(1)?,
                    series: row.get(2)?,
                    profile_name: row.get(3)?,
                    formula: row.get(4)?,
                    quantity: row.get(5)?,
                    category: row.get(6)?,
                    sort_order: row.get(7)?,
                    condition: row.get(8)?,
                    quantity_formula: row.get(9)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// 列出所有产品类型(去重)
    pub fn profile_lib_types(&self) -> Result<Vec<(String, String)>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT DISTINCT product_type, series FROM profile_lib ORDER BY product_type, series")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// 统计（量表）：汇总 + 按月序列 + 状态分布。
    /// `customer` 为空表示全部订单；否则只统计该客户。
    /// 金额一律在 SQL 里按分四舍五入聚合，避免前端累加产生浮点误差。
    pub fn stats(&self, customer: Option<&str>) -> Result<StatsResult, String> {
        let conn = self.db.lock().unwrap();
        let cust = customer.map(|c| c.trim()).filter(|c| !c.is_empty());
        // 客户量表里的「(未填客户)」分组要能真正筛出订单：
        // 订单的 customer 是 NULL 或空串，用 = '(未填客户)' 永远匹配不到。
        const EMPTY_CUST: &str = "(未填客户)";
        let where_cust = "(?1 IS NULL \
             OR (?1 = '(未填客户)' AND COALESCE(TRIM(customer),'') = '') \
             OR (?1 <> '(未填客户)' AND customer = ?1))";

        // 面积/均价只按「尺寸正常」的订单算：一条 4545555×3246456 的脏数据
        // 就能把总面积撑到上千万平米、均价压成 0，账会完全失真。
        const OK_SIZE: &str = "COALESCE(width,0) BETWEEN 300 AND 6000 AND COALESCE(height,0) BETWEEN 300 AND 3500";
        let sql = format!(
            "SELECT COUNT(*), \
                    ROUND(COALESCE(SUM(COALESCE(total_price,0)),0),2), \
                    ROUND(COALESCE(SUM(CASE WHEN {ok} THEN COALESCE(width,0)*COALESCE(height,0)/1000000.0 ELSE 0 END),0),2), \
                    COALESCE(SUM(CASE WHEN {ok} THEN 0 ELSE 1 END),0), \
                    COALESCE(SUM(CASE WHEN COALESCE(total_price,0) = 0 THEN 1 ELSE 0 END),0), \
                    ROUND(COALESCE(SUM(CASE WHEN {ok} THEN COALESCE(total_price,0) ELSE 0 END),0),2), \
                    COALESCE(SUM(CASE WHEN {ok} THEN 1 ELSE 0 END),0), \
                    ROUND(COALESCE(SUM(CASE WHEN {ok} THEN 0 ELSE COALESCE(total_price,0) END),0),2) \
             FROM orders WHERE {wc}",
            ok = OK_SIZE, wc = where_cust
        );
        let (order_count, total_amount, total_area, abnormal, zero_price, valid_amount, valid_count, abnormal_amount) = conn
            .query_row(&sql, params![cust], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, f64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, f64>(7)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        let avg_price = if total_area > 0.0 {
            (valid_amount / total_area * 100.0).round() / 100.0
        } else {
            0.0
        };

        let month_sql = format!(
                "SELECT substr(COALESCE(NULLIF(order_date,''), created_at),1,7) AS ym, \
                        COUNT(*), \
                        ROUND(COALESCE(SUM(COALESCE(total_price,0)),0),2), \
                        ROUND(COALESCE(SUM(COALESCE(width,0)*COALESCE(height,0)/1000000.0),0),2) \
                 FROM orders \
                 WHERE {wc} \
                   AND COALESCE(NULLIF(order_date,''), created_at) >= date('now','-11 months','start of month') \
                 GROUP BY ym ORDER BY ym",
            wc = where_cust);
        let mut stmt = conn
            .prepare(&month_sql)
            .map_err(|e| e.to_string())?;
        let points = stmt
            .query_map(params![cust], |r| {
                Ok(StatPoint {
                    label: r.get(0)?,
                    count: r.get(1)?,
                    amount: r.get(2)?,
                    area: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        // 补齐近 12 个月：没有订单的月份补 0，否则折线会把 1 月和 6 月直接连起来，
        // 看起来像「一直在卖」，实际中间空了好几个月。
        let points = fill_missing_months(points, 12);

        let mut stmt2 = conn
            .prepare(
                &format!(
                "SELECT status, COUNT(*), ROUND(COALESCE(SUM(COALESCE(total_price,0)),0),2) \
                 FROM orders WHERE {wc} GROUP BY status",
                wc = where_cust),
            )
            .map_err(|e| e.to_string())?;
        let by_status = stmt2
            .query_map(params![cust], |r| {
                Ok(StatPoint {
                    label: r.get(0)?,
                    count: r.get(1)?,
                    amount: r.get(2)?,
                    area: 0.0,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        Ok(StatsResult {
            summary: StatSummary {
                order_count,
                total_amount,
                total_area,
                avg_price,
                abnormal_count: abnormal,
                zero_price_count: zero_price,
                valid_count,
                valid_amount,
                abnormal_amount,
            },
            points,
            by_status,
        })
    }

    /// 客户量表用的客户清单：客户名 / 订单数 / 金额 / 面积
    pub fn stats_customers(&self) -> Result<Vec<CustomerStat>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare(
                // 面积口径必须与总量表一致（排除异常尺寸），否则客户表里的面积
                // 加起来和总量表对不上，账就错了。
                "SELECT COALESCE(NULLIF(TRIM(customer),''),'(未填客户)') AS cn, \
                        COUNT(*), \
                        ROUND(COALESCE(SUM(COALESCE(total_price,0)),0),2), \
                        ROUND(COALESCE(SUM(CASE WHEN COALESCE(width,0) BETWEEN 300 AND 6000 \
                                                AND COALESCE(height,0) BETWEEN 300 AND 3500 \
                                           THEN COALESCE(width,0)*COALESCE(height,0)/1000000.0 ELSE 0 END),0),2) \
                 FROM orders GROUP BY cn ORDER BY 3 DESC, 2 DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(CustomerStat {
                    customer: r.get(0)?,
                    order_count: r.get(1)?,
                    amount: r.get(2)?,
                    area: r.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    }

    /// 全部型材名称（去重）：异形订单手工添型材时作为候选
    pub fn profile_lib_names(&self) -> Result<Vec<String>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT DISTINCT profile_name FROM profile_lib WHERE COALESCE(profile_name,'') <> '' ORDER BY profile_name")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// 保存/更新型材库公式(用于系统设置自定义公式)
    pub fn profile_lib_save(&self, p: ProfileLib) -> Result<ProfileLib, String> {
        let conn = self.db.lock().unwrap();
        conn.execute(
            "UPDATE profile_lib SET formula=?1, quantity=?2 WHERE id=?3",
            params![p.formula, p.quantity, p.id],
        )
        .map_err(|e| e.to_string())?;
        drop(conn);
        Ok(p)
    }

    /// 解析中间变量:读 profile_lib 中 category=变量的行,按条件匹配,递归求值。
    /// 基础变量:总宽/总高/扇数/窗数/门高/墙厚/重叠边数量/单双玻/轨道类型/包套/是否有收口
    pub fn resolve_vars(
        &self,
        product_type: &str,
        series: &str,
        width: f64,
        height: f64,
        fan_count: i64,
        window_count: i64,
        door_height: f64,
        wall_thickness: f64,
        track_type: &str,
        overlap_count: i64,
        double_glass: bool,
    ) -> Result<std::collections::HashMap<String, f64>, String> {
        let mut vars: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        // 基础变量
        vars.insert("总宽".to_string(), width);
        vars.insert("总高".to_string(), height);
        vars.insert("扇数".to_string(), fan_count as f64);
        // 窗数不是用户手填项(原软件下单格式只有「窗高/门高」):由前端按型号自动推断。
        // 这里只做兜底约束,避免异常值。
        vars.insert("窗数".to_string(), if window_count < 1 { 1.0 } else { window_count as f64 });
        vars.insert("门高".to_string(), door_height);
        vars.insert("墙厚".to_string(), wall_thickness);
        vars.insert("重叠边数量".to_string(), overlap_count as f64);
        // 英文兼容
        vars.insert("W".to_string(), width);
        vars.insert("H".to_string(), height);
        vars.insert("N".to_string(), fan_count as f64);
        // 布尔型变量(0/1)
        vars.insert("单双玻".to_string(), if double_glass { 2.0 } else { 1.0 });
        vars.insert("双玻".to_string(), if double_glass { 2.0 } else { 1.0 });
        vars.insert("单玻".to_string(), if double_glass { 1.0 } else { 2.0 });
        // 包套:双包=1, 单包/无=0(条件"包套=双包"用于套线)
        vars.insert("包套".to_string(), if track_type.contains("包") { 1.0 } else { 0.0 });
        // 窗高:仅当"有上亮"(门高>0)时才 >0;门高=0 表示整扇到顶,没有上亮/固定窗
        vars.insert("窗高".to_string(), 0.0);
        // 上亮高:门高>0 时 = 总高-门高;否则必须是 0。
        // 旧写法 (height-door_height) 在门高=0 时等于总高,会误把"窗高>0"判真,
        // 生成不该有的上亮玻璃并算出负宽度 —— 已修正。
        let shangliang_base = if door_height > 0.0 { (height - door_height).max(0.0) } else { 0.0 };
        vars.insert("上亮高".to_string(), shangliang_base);
        vars.insert("含轨道的高度".to_string(), if door_height > 0.0 { door_height } else { height });

        // 读 category=变量 的定义行
        let lib = self.profile_lib_list(product_type, series)?;
        let var_rows: Vec<&ProfileLib> = lib.iter().filter(|p| p.category.as_deref() == Some("变量")).collect();

        // 迭代多轮解析(变量之间可能相互依赖,如 含轨道的高度 依赖 门高)
        for _round in 0..6 {
            let mut changed = false;
            for p in &var_rows {
                let name = p.profile_name.clone().unwrap_or_default();
                let cond = p.condition.clone().unwrap_or_default();
                let formula = p.formula.clone().unwrap_or_default();
                // 条件为空 -> 恒真;否则需条件满足(可能引用尚未解析的变量,跳过)
                if !cond.is_empty() {
                    // 条件里引用"轨道类型"等字符串变量,特殊处理
                    if !eval_string_condition(&cond, track_type, double_glass, &vars) {
                        continue;
                    }
                }
                if name.is_empty() || formula.is_empty() {
                    continue;
                }
                // 公式里若引用了尚未解析的变量,跳过本轮
                let val = eval_with_vars(&formula, &vars);
                if val == 0.0 && formula.contains("总高") && height == 0.0 {
                    // 总高为0时跳过,避免覆盖
                }
                vars.insert(name.clone(), val);
                changed = true;
            }
            if !changed {
                break;
            }
        }

        // 特殊:含轨道的高度 = 门高>0 ? 门高 : 总高(已通过变量定义覆盖,这里兜底)
        if !vars.contains_key("含轨道的高度") {
            vars.insert("含轨道的高度".to_string(), if door_height > 0.0 { door_height } else { height });
        }
        // 上亮高兜底(与基础值一致,变量定义已覆盖时保留覆盖值)
        if !vars.contains_key("上亮高") {
            vars.insert("上亮高".to_string(), shangliang_base);
        }
        // 上亮宽兜底:避免引用未定义变量时算出负尺寸
        if !vars.contains_key("上亮宽") {
            vars.insert("上亮宽".to_string(), width);
        }
        // 窗高:只有真的有上亮(门高>0 且上亮部分>0)才 >0
        let shangliang = vars.get("上亮高").copied().unwrap_or(0.0);
        if door_height > 0.0 && shangliang > 0.0 {
            vars.insert("窗高".to_string(), shangliang);
        } else {
            vars.insert("窗高".to_string(), 0.0);
        }
        Ok(vars)
    }

    /// 计算型材下料尺寸:解析中间变量后,按条件匹配求值,返回 (型材名, 长度, 数量, 分类)
    pub fn calc_profile_sizes(
        &self,
        product_type: &str,
        series: &str,
        width: f64,
        height: f64,
        fan_count: i64,
        window_count: i64,
        door_height: f64,
        wall_thickness: f64,
        track_type: &str,
        overlap_count: i64,
        double_glass: bool,
    ) -> Result<Vec<(String, f64, i64, String)>, String> {
        // 平开门没有轨道概念:强制忽略轨道参数,避免误选轨道影响下料(前端已隐藏该字段)
        let track_type = if product_type == "平开门" { "" } else { track_type };
        let vars = self.resolve_vars(
            product_type, series, width, height, fan_count,
            window_count, door_height, wall_thickness, track_type, overlap_count, double_glass,
        )?;

        let lib = self.profile_lib_list(product_type, series)?;
        let mut out = Vec::new();
        for p in lib {
            let cat = p.category.clone().unwrap_or_default();
            // 跳过"变量"类、"报价"类,以及"空公式"占位行(该型号原软件公式为空)
            if cat == "变量" || cat == "报价" || cat == "空公式" {
                continue;
            }
            let cond = p.condition.clone().unwrap_or_default();
            if !cond.is_empty() && !eval_string_condition(&cond, track_type, double_glass, &vars) {
                continue;
            }
            let formula = p.formula.clone().unwrap_or_default();
            // 数量:优先用 quantity_formula 求值,否则用 quantity 常量
            let qf = p.quantity_formula.clone().unwrap_or_default();
            let qty_val = if !qf.is_empty() {
                eval_with_vars(&qf, &vars).round() as i64
            } else {
                p.quantity.max(0)
            };
            let len = eval_with_vars(&formula, &vars);
            out.push((
                p.profile_name.clone().unwrap_or_default(),
                len,
                qty_val.max(0),
                cat,
            ));
        }
        Ok(out)
    }

    /// 计算玻璃尺寸:读 glass_lib,按条件匹配求值,返回 (玻璃名, 宽, 高, 数量)
    pub fn calc_glass_sizes(
        &self,
        product_type: &str,
        series: &str,
        width: f64,
        height: f64,
        fan_count: i64,
        window_count: i64,
        door_height: f64,
        wall_thickness: f64,
        track_type: &str,
        overlap_count: i64,
        double_glass: bool,
    ) -> Result<Vec<(String, f64, f64, i64)>, String> {
        // 平开门没有轨道概念:强制忽略轨道参数
        let track_type = if product_type == "平开门" { "" } else { track_type };
        let vars = self.resolve_vars(
            product_type, series, width, height, fan_count,
            window_count, door_height, wall_thickness, track_type, overlap_count, double_glass,
        )?;

        let items: Vec<(String, String, String, String, String)> = {
            let conn = self.db.lock().unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT glass_name, height_formula, width_formula, quantity_formula, condition \
                     FROM glass_lib WHERE product_type=?1 AND series=?2 ORDER BY sort_order, id",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![product_type, series], |row| {
                    Ok((
                        row.get::<_, String>(0).unwrap_or_default(),
                        row.get::<_, String>(1).unwrap_or_default(),
                        row.get::<_, String>(2).unwrap_or_default(),
                        row.get::<_, String>(3).unwrap_or_default(),
                        row.get::<_, String>(4).unwrap_or_default(),
                    ))
                })
                .map_err(|e| e.to_string())?;
            rows.filter_map(|x| x.ok()).collect()
        };

        let mut out = Vec::new();
        for (name, hf, wf, qf, cond) in items {
            if !cond.is_empty() && !eval_string_condition(&cond, track_type, double_glass, &vars) {
                continue;
            }
            let gh = eval_with_vars(&hf, &vars);
            let gw = eval_with_vars(&wf, &vars);
            let gq = eval_with_vars(&qf, &vars).round() as i64;
            // 返回顺序: (名称, 宽, 高, 数量)
            out.push((name, gw, gh, gq.max(1)));
        }
        Ok(out)
    }

    // ---------- 生产环节 ----------

    pub fn step_list(&self, order_id: i64) -> Result<Vec<ProductionStep>, String> {
        let conn = self.db.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, order_id, step_name, done, done_at FROM production_steps WHERE order_id=?1 ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![order_id], |row| {
                Ok(ProductionStep {
                    id: row.get(0)?,
                    order_id: row.get(1)?,
                    step_name: row.get(2)?,
                    done: row.get(3)?,
                    done_at: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn step_toggle(&self, order_id: i64, step_name: &str, done: bool) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM production_steps WHERE order_id=?1 AND step_name=?2",
                params![order_id, step_name],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists > 0 {
            conn.execute(
                "UPDATE production_steps SET done=?1, done_at=datetime('now','localtime') WHERE order_id=?2 AND step_name=?3",
                params![if done { 1 } else { 0 }, order_id, step_name],
            )
            .map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "INSERT INTO production_steps(order_id, step_name, done) VALUES(?1,?2,?3)",
                params![order_id, step_name, if done { 1 } else { 0 }],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// 通用公式求值器:支持变量(中文/英文)、四则运算 + - * /、比较 > >= < <= = !=、and/or、
/// 括号、数字、数学函数(四舍五入/取整/开方/平方/绝对值等)。
/// 采用递归下降解析,容忍空格。解析失败返回 0。
/// 变量通过 vars 映射传入;也兼容旧的 W/H/N 单字母变量(自动映射)。
pub fn eval_with_vars(expr: &str, vars: &std::collections::HashMap<String, f64>) -> f64 {
    let tokens = tokenize(expr);
    let mut parser = FormulaParser { tokens, pos: 0, vars: vars.clone() };
    let val = parser.parse_expr();
    if parser.pos == parser.tokens.len() {
        val
    } else {
        val
    }
}

/// 兼容旧接口:变量 W/H/N 直接映射
fn eval_formula(expr: &str, w: f64, h: f64, n: i64) -> f64 {
    let mut vars = std::collections::HashMap::new();
    vars.insert("W".to_string(), w);
    vars.insert("H".to_string(), h);
    vars.insert("N".to_string(), n as f64);
    vars.insert("总宽".to_string(), w);
    vars.insert("总高".to_string(), h);
    vars.insert("扇数".to_string(), n as f64);
    eval_with_vars(expr, &vars)
}

/// 判断条件表达式是否为真(用于型材/玻璃/报价的条件分支)
pub fn eval_condition(cond: &str, vars: &std::collections::HashMap<String, f64>) -> bool {
    if cond.trim().is_empty() {
        return true; // 无条件 -> 恒真
    }
    eval_with_vars(cond, vars) > 0.5
}

/// 判断含字符串变量的条件(如 "轨道类型=地轨"、"单双玻=双玻"、"包套=双包")
/// 字符串变量通过字符串匹配处理,数值变量仍走 eval_with_vars。
pub fn eval_string_condition(
    cond: &str,
    track_type: &str,
    double_glass: bool,
    vars: &std::collections::HashMap<String, f64>,
) -> bool {
    // 轨道类型别名归一:老版本下拉里的「单轨/双轨/三轨」等价映射到标准分支
    // (原软件同型号里「双轨」对应地轨扣减、「三轨」对应吊轨扣减,数值一致)
    let track_type = match track_type {
        "单轨" | "双轨" => "地轨",
        "三轨" => "吊轨",
        _ => track_type,
    };
    let cond = cond.trim();
    if cond.is_empty() {
        return true;
    }

    // 拆分 and / or
    let parts: Vec<&str> = cond.split("and").map(|s| s.trim()).collect();
    // 如果只有一个部分,检查是否含 or
    if parts.len() == 1 {
        let or_parts: Vec<&str> = cond.split("or").map(|s| s.trim()).collect();
        if or_parts.len() > 1 {
            return or_parts.iter().any(|p| eval_string_condition(p, track_type, double_glass, vars));
        }
    }

    // and 语义:所有子条件都满足
    if parts.len() > 1 {
        return parts.iter().all(|p| eval_string_condition(p, track_type, double_glass, vars));
    }

    // 单条件:处理 "X=Y" / "X>N" / "X<=N" 等
    let c = cond;

    // 数值比较运算符(按长度优先: >= <= != 先于 > < =)
    let cmp_ops: &[&str] = &[">=", "<=", "!=", ">", "<", "="];
    for op in cmp_ops {
        if let Some(idx) = c.find(op) {
            // 排除字符串相等里的 "=" 被误当数值(如 轨道类型=地轨, right 非数字)
            let left = c[..idx].trim();
            let right = c[idx + op.len()..].trim();
            let right_val = right.trim_matches('"').trim_matches('\'');

            // 字符串变量处理(轨道类型/单双玻/包套)
            let left_str = match left {
                "轨道类型" => Some(track_type.to_string()),
                "单双玻" | "双玻" => Some(if double_glass { "双玻".to_string() } else { "单玻".to_string() }),
                "包套" => Some(if track_type.contains("包") { "双包".to_string() } else { "".to_string() }),
                _ => None,
            };
            if let Some(lv_str) = left_str {
                // 字符串比较
                if *op == "=" {
                    return lv_str == right_val;
                }
                if *op == "!=" {
                    return lv_str != right_val;
                }
                return false;
            }

            // 数值比较
            let lv = vars.get(left).copied().unwrap_or_else(|| eval_with_vars(left, vars));
            if let Ok(rv) = right_val.parse::<f64>() {
                return match *op {
                    ">" => lv > rv,
                    ">=" => lv >= rv,
                    "<" => lv < rv,
                    "<=" => lv <= rv,
                    "!=" => (lv - rv).abs() > 1e-6,
                    "=" => (lv - rv).abs() < 1e-6,
                    _ => false,
                };
            }
            // right 非数字:字符串比较兜底
            return match *op {
                "=" => lv > 0.5,
                "!=" => lv <= 0.5,
                _ => false,
            };
        }
    }

    // 兜底:直接数值求值
    eval_with_vars(c, vars) > 0.5
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String), // 变量名(中文/英文)
    Op(String),    // 运算符: + - * / > >= < <= = == != and or
    LParen,
    RParen,
    Comma,
    Func(String),
}

/// 支持的函数名(中文+英文别名),统一映射到内部名
const FUNC_NAMES: &[(&str, &str)] = &[
    ("四舍五入", "round"),
    ("取整", "trunc"),
    ("整数", "trunc"),
    ("向上取整", "ceil"),
    ("向下取整", "floor"),
    ("绝对值", "abs"),
    ("开方", "sqrt"),
    ("平方", "pow"),
    ("次方", "pow"),
    ("round", "round"),
    ("trunc", "trunc"),
    ("int", "trunc"),
    ("ceil", "ceil"),
    ("floor", "floor"),
    ("abs", "abs"),
    ("sqrt", "sqrt"),
    ("pow", "pow"),
];

/// 逻辑/比较运算符关键字(按长度降序,先匹配长的)
const LOGIC_OPS: &[&str] = &["and", "or", "AND", "OR"];

fn tokenize(expr: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => { i += 1; }
            '0'..='9' | '.' => {
                let mut j = i;
                while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                    j += 1;
                }
                let s: String = chars[i..j].iter().collect();
                if let Ok(v) = s.parse::<f64>() {
                    out.push(Tok::Num(v));
                }
                i = j;
            }
            '+' | '-' | '*' | '/' => {
                out.push(Tok::Op(c.to_string()));
                i += 1;
            }
            '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Op(">=".to_string()));
                    i += 2;
                } else {
                    out.push(Tok::Op(">".to_string()));
                    i += 1;
                }
            }
            '<' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Op("<=".to_string()));
                    i += 2;
                } else {
                    out.push(Tok::Op("<".to_string()));
                    i += 1;
                }
            }
            '=' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Op("=".to_string()));
                    i += 2;
                } else {
                    out.push(Tok::Op("=".to_string()));
                    i += 1;
                }
            }
            '!' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Op("!=".to_string()));
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '(' => { out.push(Tok::LParen); i += 1; }
            ')' => { out.push(Tok::RParen); i += 1; }
            ',' | '，' => { out.push(Tok::Comma); i += 1; }
            _ if c.is_ascii_alphabetic() || (!c.is_ascii() && c != '，') => {
                // 尝试匹配函数名(中文或英文),从当前位置向前匹配最长者
                let rest: String = chars[i..].iter().collect();
                let mut matched: Option<(&str, &str)> = None;
                for (name, target) in FUNC_NAMES {
                    if rest.starts_with(name)
                        && matched.map_or(true, |(n, _)| name.chars().count() > n.chars().count())
                    {
                        matched = Some((name, target));
                    }
                }
                if let Some((name, target)) = matched {
                    out.push(Tok::Func(target.to_string()));
                    i += name.chars().count();
                } else {
                    // 逻辑关键字 and/or
                    let mut logic: Option<&str> = None;
                    for kw in LOGIC_OPS {
                        if rest.starts_with(kw) {
                            logic = Some(kw);
                            break;
                        }
                    }
                    if let Some(kw) = logic {
                        out.push(Tok::Op(kw.to_ascii_lowercase()));
                        i += kw.len();
                    } else {
                        // 变量名(中文或英文标识符):连续字母或中文字符
                        let mut j = i;
                        while j < chars.len()
                            && (chars[j].is_ascii_alphanumeric() || (!chars[j].is_ascii() && chars[j] != '，'))
                            && !is_delim(chars[j])
                        {
                            j += 1;
                        }
                        let name: String = chars[i..j].iter().collect();
                        out.push(Tok::Ident(name));
                        i = j;
                    }
                }
            }
            _ => { i += 1; } // 忽略未知字符
        }
    }
    out
}

/// 判断字符是否为需要停止变量名匹配的分隔符(运算符/括号等)
fn is_delim(c: char) -> bool {
    matches!(c, '+' | '-' | '*' | '/' | '>' | '<' | '=' | '!' | '(' | ')' | ',' | '，')
}

struct FormulaParser {
    tokens: Vec<Tok>,
    pos: usize,
    vars: std::collections::HashMap<String, f64>,
}

impl FormulaParser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    /// 表达式: and/or 逻辑层
    fn parse_expr(&mut self) -> f64 {
        let mut v = self.parse_cmp();
        loop {
            match self.peek() {
                Some(Tok::Op(op)) if op == "and" => {
                    self.pos += 1;
                    let r = self.parse_cmp();
                    v = if v > 0.5 && r > 0.5 { 1.0 } else { 0.0 };
                }
                Some(Tok::Op(op)) if op == "or" => {
                    self.pos += 1;
                    let r = self.parse_cmp();
                    v = if v > 0.5 || r > 0.5 { 1.0 } else { 0.0 };
                }
                _ => break,
            }
        }
        v
    }

    /// 比较层
    fn parse_cmp(&mut self) -> f64 {
        let mut v = self.parse_add();
        loop {
            match self.peek() {
                Some(Tok::Op(op)) if is_cmp_op(op) => {
                    let op = op.clone();
                    self.pos += 1;
                    let r = self.parse_add();
                    v = match op.as_str() {
                        ">" => (v > r) as i64 as f64,
                        ">=" => (v >= r) as i64 as f64,
                        "<" => (v < r) as i64 as f64,
                        "<=" => (v <= r) as i64 as f64,
                        "=" => (v == r) as i64 as f64,
                        "!=" => (v != r) as i64 as f64,
                        _ => v,
                    };
                }
                _ => break,
            }
        }
        v
    }

    /// 加减层
    fn parse_add(&mut self) -> f64 {
        let mut v = self.parse_term();
        while let Some(Tok::Op(op)) = self.peek() {
            match op.as_str() {
                "+" => { self.pos += 1; v += self.parse_term(); }
                "-" => { self.pos += 1; v -= self.parse_term(); }
                _ => break,
            }
        }
        v
    }

    /// 乘除层
    fn parse_term(&mut self) -> f64 {
        let mut v = self.parse_factor();
        while let Some(Tok::Op(op)) = self.peek() {
            match op.as_str() {
                "*" => { self.pos += 1; v *= self.parse_factor(); }
                "/" => {
                    self.pos += 1;
                    let d = self.parse_factor();
                    if d.abs() > 1e-12 { v /= d; }
                }
                _ => break,
            }
        }
        v
    }

    fn parse_factor(&mut self) -> f64 {
        match self.next() {
            Some(Tok::Num(x)) => x,
            Some(Tok::Ident(name)) => {
                // 变量取值;支持单字母 W/H/N 及中文变量
                self.vars.get(&name).copied().unwrap_or_else(|| {
                    let upper = name.to_ascii_uppercase();
                    self.vars.get(&upper).copied().unwrap_or(0.0)
                })
            }
            Some(Tok::LParen) => {
                let v = self.parse_expr();
                self.next(); // 消费 )
                v
            }
            Some(Tok::Func(name)) => {
                if let Some(Tok::LParen) = self.peek() {
                    self.next(); // 消费 '('
                    let mut args = Vec::new();
                    if let Some(Tok::RParen) = self.peek() {
                        self.next(); // 空参数 ()
                    } else {
                        loop {
                            args.push(self.parse_expr());
                            match self.peek() {
                                Some(Tok::Comma) => { self.next(); }
                                _ => break,
                            }
                        }
                        self.next(); // 消费 ')'
                    }
                    apply_func(&name, &args)
                } else {
                    0.0
                }
            }
            Some(Tok::Op(op)) if op == "-" => -self.parse_factor(), // 一元负号
            Some(Tok::Op(op)) if op == "+" => self.parse_factor(),
            _ => 0.0,
        }
    }
}

fn is_cmp_op(op: &str) -> bool {
    matches!(op, ">" | ">=" | "<" | "<=" | "=" | "!=")
}

/// 执行内置数学函数
fn apply_func(name: &str, args: &[f64]) -> f64 {
    match name {
        "round" => {
            let x = args.first().copied().unwrap_or(0.0);
            let n = args.get(1).copied().unwrap_or(0.0) as i32;
            let f = 10f64.powi(n);
            (x * f).round() / f
        }
        "trunc" => args.first().copied().unwrap_or(0.0).trunc(),
        "ceil" => args.first().copied().unwrap_or(0.0).ceil(),
        "floor" => args.first().copied().unwrap_or(0.0).floor(),
        "abs" => args.first().copied().unwrap_or(0.0).abs(),
        "sqrt" => args.first().copied().unwrap_or(0.0).max(0.0).sqrt(),
        "pow" => {
            let base = args.first().copied().unwrap_or(0.0);
            let exp = args.get(1).copied().unwrap_or(1.0);
            base.powf(exp)
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod formula_tests {
    use super::{eval_condition, eval_formula, eval_string_condition, eval_with_vars};
    use std::collections::HashMap;

    #[test]
    fn test_math_funcs() {
        // 四舍五入(1000/3, 2) = 333.33
        assert!((eval_formula("四舍五入(1000/3, 2)", 0.0, 0.0, 0) - 333.33).abs() < 0.001);
        // 求平方(10, 2) = 100
        assert!((eval_formula("平方(10, 2)", 0.0, 0.0, 0) - 100.0).abs() < 1e-9);
        // 绝对值(-100) = 100
        assert!((eval_formula("绝对值(-100)", 0.0, 0.0, 0) - 100.0).abs() < 1e-9);
        // 开方(100) = 10
        assert!((eval_formula("开方(100)", 0.0, 0.0, 0) - 10.0).abs() < 1e-9);
        // round(W/2, 1), W=1730 -> 865.0
        assert!((eval_formula("round(W/2, 1)", 1730.0, 0.0, 0) - 865.0).abs() < 1e-9);
        // 取整(1056.65) = 1056
        assert!((eval_formula("取整(1056.65)", 0.0, 0.0, 0) - 1056.0).abs() < 1e-9);
        // 嵌套: 四舍五入(W/2, 1)+10, W=1730 -> 875
        assert!((eval_formula("四舍五入(W/2, 1)+10", 1730.0, 0.0, 0) - 875.0).abs() < 1e-9);
    }

    #[test]
    fn test_chinese_vars_and_conditions() {
        let mut vars = HashMap::new();
        vars.insert("总宽".to_string(), 1730.0);
        vars.insert("总高".to_string(), 2140.0);
        vars.insert("扇数".to_string(), 2.0);
        vars.insert("窗数".to_string(), 2.0);
        vars.insert("门高".to_string(), 0.0);

        // 中文变量四则运算: 总宽-40 = 1690
        assert!((eval_with_vars("总宽-40", &vars) - 1690.0).abs() < 1e-9);
        // 上亮玻璃宽: 窗数=2 -> (总宽-30-30-30)/2 = (1730-90)/2 = 820
        assert!((eval_with_vars("(总宽-30-30-30)/2", &vars) - 820.0).abs() < 1e-9);
        // 上下方: (总宽-26+16)/2+28 = (1730-10)/2+28 = 860+28 = 888
        assert!((eval_with_vars("(总宽-26+16)/2+28", &vars) - 888.0).abs() < 1e-9);
        // 基础价: 总高*总宽/1000000 = 2140*1730/1000000 = 3.7022
        assert!((eval_with_vars("总高*总宽/1000000", &vars) - 3.7022).abs() < 0.0001);

        // 条件判断: 扇数=2 -> 1
        assert!(eval_condition("扇数=2", &vars));
        assert!(!eval_condition("扇数=3", &vars));
        // 门高>0 -> 0 (门高=0)
        assert!(!eval_condition("门高>0", &vars));
        // and/or: 扇数=2 and 门高>0 -> false; 扇数=2 or 门高>0 -> true
        assert!(!eval_condition("扇数=2 and 门高>0", &vars));
        assert!(eval_condition("扇数=2 or 门高>0", &vars));
    }

    #[test]
    fn test_series_branch_conditions() {
        let mut vars = HashMap::new();
        vars.insert("总宽".to_string(), 1730.0);
        vars.insert("扇数".to_string(), 4.0);
        vars.insert("重叠边数量".to_string(), 2.0);

        // 上下方: 扇数=4 -> (总宽-45+60+60)/4 = (1730+75)/4 = 451.25
        assert!((eval_with_vars("(总宽-45+60+60)/4", &vars) - 451.25).abs() < 1e-9);
        // 条件: 扇数=3 and 重叠边数量=2 -> false (扇数=4)
        assert!(!eval_condition("扇数=3 and 重叠边数量=2", &vars));
        assert!(eval_condition("扇数=4", &vars));
    }

    #[test]
    fn test_string_conditions() {
        let vars = HashMap::new();

        // 轨道类型=地轨 -> true
        assert!(eval_string_condition("轨道类型=地轨", "地轨", true, &vars));
        assert!(!eval_string_condition("轨道类型=吊轨", "地轨", true, &vars));
        // 单双玻=双玻 -> true (double_glass=true)
        assert!(eval_string_condition("单双玻=双玻", "地轨", true, &vars));
        assert!(!eval_string_condition("单双玻=双玻", "地轨", false, &vars));
        // 门高>0 -> 需要 vars 里有门高
        let mut v2 = HashMap::new();
        v2.insert("门高".to_string(), 0.0);
        assert!(!eval_string_condition("门高>0", "地轨", true, &v2));
        v2.insert("门高".to_string(), 500.0);
        assert!(eval_string_condition("门高>0", "地轨", true, &v2));
        // 扇数=4 or 是否有收口=1
        let mut v3 = HashMap::new();
        v3.insert("扇数".to_string(), 4.0);
        v3.insert("是否有收口".to_string(), 0.0);
        assert!(eval_string_condition("扇数=4 or 是否有收口=1", "地轨", true, &v3));
    }
}

/// 订单保存时自动把客户写进客户表。
/// 同名视为同一客户：只补空的电话/地址，不覆盖已有内容；返回客户 id。
fn upsert_customer(
    conn: &Connection,
    name: Option<&str>,
    phone: Option<&str>,
    addr: Option<&str>,
) -> Option<(i64, bool)> {
    let name = name.map(|s| s.trim()).filter(|s| !s.is_empty())?;
    let exist: Option<i64> = conn
        .query_row(
            // 客户名忽略大小写与首尾空格视为同一客户（「张三」和「张三 」不该分成两条）
            "SELECT id FROM customers WHERE TRIM(name) = ?1 COLLATE NOCASE",
            params![name],
            |r| r.get(0),
        )
        .ok();
    match exist {
        Some(id) => {
            conn.execute(
                "UPDATE customers SET \
                   phone = CASE WHEN COALESCE(phone,'')='' THEN ?2 ELSE phone END, \
                   terminal_addr = CASE WHEN COALESCE(terminal_addr,'')='' THEN ?3 ELSE terminal_addr END \
                 WHERE id = ?1",
                params![id, phone, addr],
            )
            .ok();
            Some((id, false))
        }
        None => {
            conn.execute(
                "INSERT INTO customers(name, phone, terminal_addr) VALUES(?1, ?2, ?3)",
                params![name, phone, addr],
            )
            .ok()?;
            Some((conn.last_insert_rowid(), true))
        }
    }
}

/// 历史订单补录客户（幂等）：
/// 旧版本保存订单时没有同步客户表，导致客户管理里是空的。
/// 启动时按订单里的客户名补一次，已有同名客户只补空字段，不覆盖。
pub fn backfill_customers_from_orders(conn: &Connection) -> Result<usize, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, TRIM(customer), terminal_phone, terminal_addr FROM orders \
             WHERE COALESCE(TRIM(customer),'') <> '' ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut added = 0usize;
    for (oid, name, phone, addr) in rows {
        if let Some((cid, created)) =
            upsert_customer(conn, Some(&name), phone.as_deref(), addr.as_deref())
        {
            if created {
                added += 1;
            }
            conn.execute(
                "UPDATE orders SET customer_id=?1 WHERE id=?2 AND (customer_id IS NULL OR customer_id<>?1)",
                params![cid, oid],
            )
            .ok();
        }
    }
    Ok(added)
}

/// 近 n 个月的 ym 列表（含当月，升序）：用于把没有订单的月份补成 0
fn last_months(n: usize) -> Vec<String> {
    let now = chrono::Local::now();
    let mut y = chrono::Datelike::year(&now);
    let mut m = chrono::Datelike::month(&now) as i32;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(format!("{:04}-{:02}", y, m));
        m -= 1;
        if m == 0 {
            m = 12;
            y -= 1;
        }
    }
    out.reverse();
    out
}

/// 把按月结果补齐成固定的 n 个月（缺失月 = 0），保证折线连续
fn fill_missing_months(points: Vec<StatPoint>, n: usize) -> Vec<StatPoint> {
    let mut map: std::collections::HashMap<String, StatPoint> =
        points.into_iter().map(|p| (p.label.clone(), p)).collect();
    last_months(n)
        .into_iter()
        .map(|ym| {
            map.remove(&ym).unwrap_or(StatPoint {
                label: ym,
                count: 0,
                amount: 0.0,
                area: 0.0,
            })
        })
        .collect()
}

/// 量表：按月/按状态的统计点
#[derive(Debug, serde::Serialize)]
pub struct StatPoint {
    pub label: String,
    pub count: i64,
    pub amount: f64,
    pub area: f64,
}

/// 量表：汇总
#[derive(Debug, serde::Serialize)]
pub struct StatSummary {
    pub order_count: i64,
    pub total_amount: f64,
    pub total_area: f64,
    pub avg_price: f64,
    pub abnormal_count: i64,
    pub zero_price_count: i64,
    /// 尺寸正常、参与面积/均价计算的订单数
    pub valid_count: i64,
    /// 尺寸正常订单的金额合计（均价 = valid_amount / total_area）
    pub valid_amount: f64,
    /// 尺寸异常订单的金额（不计入均价，但要能让 total_amount 对得上账）
    pub abnormal_amount: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct StatsResult {
    pub summary: StatSummary,
    pub points: Vec<StatPoint>,
    pub by_status: Vec<StatPoint>,
}

/// 客户量表的一行
#[derive(Debug, serde::Serialize)]
pub struct CustomerStat {
    pub customer: String,
    pub order_count: i64,
    pub amount: f64,
    pub area: f64,
}
