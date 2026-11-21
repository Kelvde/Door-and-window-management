// src-tauri/src/model.rs
// 数据模型与序列化结构(与设计文档数据表字段一致)
// 合规说明:构件关系采用关系型父子层级表(parent_id / path / 递归 CTE),
// 不使用无向图 / 邻接表 / 边匹配;不实现"预设样板自动匹配 -> 算价"自动链路。

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Order {
    pub id: i64,
    pub order_no: String,
    pub contract_no: Option<String>,
    pub customer: Option<String>,
    pub customer_id: Option<i64>,
    pub terminal_addr: Option<String>,
    pub terminal_phone: Option<String>,
    pub material_type: Option<String>,
    pub product_name: Option<String>,
    pub color: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub face_glass: Option<String>,
    pub back_glass: Option<String>,
    pub glass_layer: Option<String>,
    pub face_glass_w: Option<f64>,
    pub face_glass_h: Option<f64>,
    pub back_glass_w: Option<f64>,
    pub back_glass_h: Option<f64>,
    pub style: Option<String>,
    pub remark: Option<String>,
    pub total_price: f64,
    pub unit_price: Option<f64>,
    pub status: String,
    pub mark: Option<String>,
    pub contract_status: Option<String>,
    pub order_date: Option<String>,
    pub delivery_date: Option<String>,
    pub inbound_date: Option<String>,
    pub outbound_date: Option<String>,
    pub salesman: Option<String>,
    pub operator: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    // 下料参数（编辑订单后重算型材/玻璃要用）
    pub fan_count: Option<i64>,
    pub track_type: Option<String>,
    pub door_height: Option<f64>,
    pub wall_thickness: Option<f64>,
    /// 生产环节进度汇总(如 "已完成:下料/组框; 未完成:玻璃" ),列表展示用
    #[serde(default)]
    pub step_summary: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct OrderInput {
    pub id: Option<i64>,
    pub order_no: String,
    pub contract_no: Option<String>,
    pub customer: Option<String>,
    pub customer_id: Option<i64>,
    pub terminal_addr: Option<String>,
    pub terminal_phone: Option<String>,
    pub material_type: Option<String>,
    pub product_name: Option<String>,
    pub color: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub face_glass: Option<String>,
    pub back_glass: Option<String>,
    pub glass_layer: Option<String>,
    pub face_glass_w: Option<f64>,
    pub face_glass_h: Option<f64>,
    pub back_glass_w: Option<f64>,
    pub back_glass_h: Option<f64>,
    pub style: Option<String>,
    pub remark: Option<String>,
    pub total_price: Option<f64>,
    // 单价（元/㎡）：总价 = 面积 × 单价，自动算；不填则总价手填
    #[serde(default)]
    pub unit_price: Option<f64>,
    pub status: Option<String>,
    pub mark: Option<String>,
    pub contract_status: Option<String>,
    pub order_date: Option<String>,
    pub delivery_date: Option<String>,
    pub inbound_date: Option<String>,
    pub outbound_date: Option<String>,
    pub salesman: Option<String>,
    pub operator: Option<String>,
    // 下料参数：编辑订单后重算型材/玻璃必须用这几项，否则按默认值算错
    #[serde(default)]
    pub fan_count: Option<i64>,
    #[serde(default)]
    pub track_type: Option<String>,
    #[serde(default)]
    pub door_height: Option<f64>,
    #[serde(default)]
    pub wall_thickness: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub terminal_addr: Option<String>,
    pub contact: Option<String>,
    pub remark: Option<String>,
    pub brand: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CustomerInput {
    pub id: Option<i64>,
    pub name: String,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub terminal_addr: Option<String>,
    pub contact: Option<String>,
    pub remark: Option<String>,
    pub brand: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Contract {
    pub id: i64,
    pub contract_no: String,
    pub contract_date: Option<String>,
    pub customer: Option<String>,
    pub terminal_addr: Option<String>,
    pub total_price: f64,
    pub paid_amount: f64,
    pub erase_zero: f64,
    pub balance: f64,
    pub order_count: i64,
    pub pay_progress: Option<String>,
    pub inbound_progress: Option<String>,
    pub outbound_progress: Option<String>,
    pub product_name: Option<String>,
    pub logistics: Option<String>,
    pub logistics_no: Option<String>,
    pub mark: Option<String>,
    pub remark: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ContractInput {
    pub id: Option<i64>,
    pub contract_no: String,
    pub contract_date: Option<String>,
    pub customer: Option<String>,
    pub terminal_addr: Option<String>,
    pub total_price: Option<f64>,
    pub paid_amount: Option<f64>,
    pub erase_zero: Option<f64>,
    pub balance: Option<f64>,
    pub order_count: Option<i64>,
    pub pay_progress: Option<String>,
    pub inbound_progress: Option<String>,
    pub outbound_progress: Option<String>,
    pub product_name: Option<String>,
    pub logistics: Option<String>,
    pub logistics_no: Option<String>,
    pub mark: Option<String>,
    pub remark: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Component {
    #[serde(default)]
    pub id: i64,
    pub order_id: i64,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub spec: Option<String>,
    #[serde(default)]
    pub material_type: Option<String>,
    #[serde(default)]
    pub orig_width: Option<f64>,
    #[serde(default)]
    pub orig_height: Option<f64>,
    #[serde(default)]
    pub actual_width: Option<f64>,
    #[serde(default)]
    pub actual_height: Option<f64>,
    #[serde(default)]
    pub quantity: i64,
    #[serde(default)]
    pub sort_order: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Profile {
    #[serde(default)]
    pub id: i64,
    pub order_id: i64,
    pub profile_name: Option<String>,
    pub length: Option<f64>,
    pub quantity: i64,
    pub category: Option<String>,
    pub remark: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Glass {
    #[serde(default)]
    pub id: i64,
    pub order_id: i64,
    pub glass_name: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub quantity: i64,
    pub remark: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Accessory {
    #[serde(default)]
    pub id: i64,
    pub order_id: i64,
    pub acc_name: Option<String>,
    pub quantity: i64,
    pub remark: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProductionStep {
    #[serde(default)]
    pub id: i64,
    pub order_id: i64,
    pub step_name: String,
    pub done: i64,
    pub done_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ProductPreset {
    #[serde(default)]
    pub id: i64,
    pub name: Option<String>,
    pub category: Option<String>,
    pub material_type: Option<String>,
    pub style: Option<String>,
    pub color: Option<String>,
    pub face_glass: Option<String>,
    pub back_glass: Option<String>,
    pub remark: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GlassType {
    #[serde(default)]
    pub id: i64,
    pub category: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProfileLib {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub product_type: Option<String>,
    #[serde(default)]
    pub series: Option<String>,
    #[serde(default)]
    pub profile_name: Option<String>,
    #[serde(default)]
    pub formula: Option<String>,
    #[serde(default)]
    pub quantity: i64,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub quantity_formula: Option<String>,
}

/// 玻璃公式库项
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GlassLib {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub product_type: Option<String>,
    #[serde(default)]
    pub series: Option<String>,
    #[serde(default)]
    pub glass_name: Option<String>,
    #[serde(default)]
    pub height_formula: Option<String>,
    #[serde(default)]
    pub width_formula: Option<String>,
    #[serde(default)]
    pub quantity_formula: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub sort_order: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CheckRow {
    #[serde(flatten)]
    pub comp: Component,
    pub abnormal: bool,
}

/// 打印/尺寸核对的完整核对项(玻璃尺寸、总宽高、型材、产品、玻璃类型)
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CheckDetail {
    pub order_id: i64,
    pub order_no: String,
    pub customer: Option<String>,
    /// 总宽(mm)
    pub width: Option<f64>,
    /// 总高(mm)
    pub height: Option<f64>,
    /// 玻璃层数(单玻/双玻)
    pub glass_layer: Option<String>,
    /// 产品名称(核对产品是否选错)
    pub product_name: Option<String>,
    /// 料型(核对型材是否选错)
    pub material_type: Option<String>,
    /// 款式
    pub style: Option<String>,
    /// 颜色
    pub color: Option<String>,
    /// 面玻(玻璃类型)
    pub face_glass: Option<String>,
    /// 背玻(玻璃类型)
    pub back_glass: Option<String>,
    /// 面玻尺寸宽(mm)
    pub face_glass_w: Option<f64>,
    /// 面玻尺寸高(mm)
    pub face_glass_h: Option<f64>,
    /// 背玻尺寸宽(mm)
    pub back_glass_w: Option<f64>,
    /// 背玻尺寸高(mm)
    pub back_glass_h: Option<f64>,
    /// 型材明细(核对型材)
    pub profiles: Vec<Profile>,
    /// 玻璃明细(核对玻璃)
    pub glasses: Vec<Glass>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub permissions: Vec<String>,
    pub hooks: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackupRecord {
    pub id: i64,
    pub backup_type: String,
    pub file_path: String,
    pub file_size: i64,
    pub created_at: Option<String>,
    pub status: Option<String>,
    pub name: String,
    pub exists: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Meta {
    pub name: String,
    pub version: String,
    pub backup_types: Vec<String>,
    pub doc_types: Vec<String>,
    pub data_dir: String,
    pub platform: String,
    pub company_name: String,
    pub operator_name: String,
}

/// 螺丝规格规则(下料单用):按门类型+型号系列+料型单独分类,数量/型号可编辑
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScrewRule {
    pub product_type: String,
    pub series: String,
    pub material_type: String,
    pub qty: i64,
    pub spec: String,
    pub remark: String,
}

/// 草稿缓存记录(关闭软件前把新增订单未编完的数据缓存,放在备份目录下独立 drafts 文件夹)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DraftRecord {
    pub name: String,
    pub order_no: String,
    pub customer: String,
    pub saved_at: Option<String>,
    pub size: i64,
}
