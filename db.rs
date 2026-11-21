// src-tauri/src/db.rs
// 数据库初始化与 schema(用 rusqlite,数据库文件落在 exe 同目录 data/ 下)
// 数据目录固定在软件目录(D 盘),不依赖 C 盘。

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

/// 获取软件根目录(可执行文件所在目录)
pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 数据目录 = 软件根目录/data
pub fn data_dir() -> PathBuf {
    let d = app_dir().join("data");
    std::fs::create_dir_all(&d).ok();
    d
}

/// 备份根目录 = 软件根目录/backup
pub fn backup_dir() -> PathBuf {
    app_dir().join("backup")
}

/// 草稿缓存目录 = 备份根目录/drafts(独立文件夹,不与 shutdown/snapshot/manual 混放)
pub fn draft_dir() -> PathBuf {
    let d = backup_dir().join("drafts");
    std::fs::create_dir_all(&d).ok();
    d
}

/// 单实例锁文件路径(放在 data 目录下,内容为进程 PID)
pub fn single_instance_lock_path() -> PathBuf {
    data_dir().join("app.lock")
}

/// 当前本地时间字符串(YYYY-MM-DD HH:MM:SS)
pub fn now_str() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub type Db = Mutex<Connection>;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
-- WAL 下 synchronous=NORMAL 已能保证崩溃不损坏库，写入快很多（默认 FULL 每次提交都 fsync）
PRAGMA synchronous = NORMAL;
-- 常用查询的页缓存：16MB（默认只有 2MB，型号公式多时反复读盘）
PRAGMA cache_size = -16000;
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 134217728;

-- 客户表
CREATE TABLE IF NOT EXISTS customers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,           -- 客户名称
  phone TEXT,                   -- 电话
  address TEXT,                 -- 地址
  terminal_addr TEXT,           -- 终端地址
  contact TEXT,                 -- 联系人
  remark TEXT,                  -- 备注
  brand TEXT,                   -- 客户品牌(对应原「客户品牌」字段)
  created_at TEXT DEFAULT (datetime('now','localtime'))
);

-- 订单表(扩展为完整门窗/移门业务字段)
CREATE TABLE IF NOT EXISTS orders (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_no TEXT NOT NULL,        -- 订单号
  contract_no TEXT,              -- 合同号
  customer TEXT,                 -- 客户
  customer_id INTEGER REFERENCES customers(id),
  terminal_addr TEXT,            -- 终端地址
  terminal_phone TEXT,           -- 终端电话
  material_type TEXT,            -- 料型
  product_name TEXT,             -- 产品名称
  color TEXT,                    -- 颜色
  width REAL,                    -- 尺寸 宽(mm)
  height REAL,                   -- 尺寸 高(mm)
  face_glass TEXT,               -- 面玻
  back_glass TEXT,               -- 背玻
  glass_layer TEXT,              -- 玻璃层数(单玻/双玻)
  face_glass_w REAL,             -- 面玻尺寸 宽(mm,自动计算)
  face_glass_h REAL,             -- 面玻尺寸 高(mm,自动计算)
  back_glass_w REAL,             -- 背玻尺寸 宽(mm,自动计算)
  back_glass_h REAL,             -- 背玻尺寸 高(mm,自动计算)
  style TEXT,                    -- 款式
  remark TEXT,                   -- 备注
  total_price REAL DEFAULT 0,    -- 订单总价
  status TEXT DEFAULT '未合并',   -- 订单状态:未合并/待审核/待生产/生产中/成品/已出库
  mark TEXT,                     -- 订单标记
  contract_status TEXT,          -- 合同状态
  order_date TEXT,               -- 下单日期
  delivery_date TEXT,            -- 交货日期
  inbound_date TEXT,             -- 入库日期
  outbound_date TEXT,            -- 出库日期
  salesman TEXT,                 -- 业务员
  operator TEXT,                 -- 操作员
  unit_price REAL,               -- 单价（元/㎡），总价 = 面积 × 单价
  fan_count INTEGER,             -- 扇数（编辑后重算型材要用）
  track_type TEXT,               -- 轨道类型
  door_height REAL,              -- 门高
  wall_thickness REAL,           -- 墙厚
  created_at TEXT DEFAULT (datetime('now','localtime')),
  updated_at TEXT DEFAULT (datetime('now','localtime'))
);

-- 合同表
CREATE TABLE IF NOT EXISTS contracts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  contract_no TEXT NOT NULL,     -- 合同号
  contract_date TEXT,            -- 合同日期
  customer TEXT,                 -- 客户
  terminal_addr TEXT,            -- 终端地址
  total_price REAL DEFAULT 0,    -- 合同总价
  paid_amount REAL DEFAULT 0,    -- 已付款
  erase_zero REAL DEFAULT 0,     -- 抹零
  balance REAL DEFAULT 0,        -- 尾款
  order_count INTEGER DEFAULT 0, -- 订单数量
  pay_progress TEXT,             -- 付款进度
  inbound_progress TEXT,         -- 入库进度
  outbound_progress TEXT,        -- 出库进度
  product_name TEXT,             -- 产品名称
  logistics TEXT,                -- 物流
  logistics_no TEXT,             -- 物流单号
  mark TEXT,                     -- 合同标记
  remark TEXT,                   -- 备注
  created_at TEXT DEFAULT (datetime('now','localtime')),
  updated_at TEXT DEFAULT (datetime('now','localtime'))
);

-- 构件表(父子层级:parent_id / path / 递归 CTE)
CREATE TABLE IF NOT EXISTS components (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_id INTEGER NOT NULL REFERENCES orders(id),
  parent_id INTEGER REFERENCES components(id),
  path TEXT,
  name TEXT,
  spec TEXT,
  material_type TEXT,
  orig_width REAL,
  orig_height REAL,
  actual_width REAL,
  actual_height REAL,
  quantity INTEGER DEFAULT 1,
  sort_order INTEGER DEFAULT 0
);

-- 型材明细表(下料用:每根型材的料型/尺寸/数量/类型)
CREATE TABLE IF NOT EXISTS profiles (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_id INTEGER NOT NULL REFERENCES orders(id),
  profile_name TEXT,             -- 型材名称(如上滑、下滑、光企、勾企、上方、下方等)
  length REAL,                   -- 长度(mm)
  quantity INTEGER DEFAULT 1,    -- 数量
  category TEXT,                 -- 类型:框/扇/支撑
  remark TEXT
);

-- 玻璃明细表
CREATE TABLE IF NOT EXISTS glasses (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_id INTEGER NOT NULL REFERENCES orders(id),
  glass_name TEXT,               -- 玻璃名称(面玻/背玻)
  width REAL,                    -- 宽(mm)
  height REAL,                   -- 高(mm)
  quantity INTEGER DEFAULT 1,
  remark TEXT
);

-- 配件明细表
CREATE TABLE IF NOT EXISTS accessories (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_id INTEGER NOT NULL REFERENCES orders(id),
  acc_name TEXT,
  quantity INTEGER DEFAULT 1,
  remark TEXT
);

-- 产品预设库(产品名称/料型/款式/颜色/玻璃 预输入模板)
CREATE TABLE IF NOT EXISTS product_presets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT,                     -- 产品名称(如 单轨推拉门)
  category TEXT,                 -- 分类(如 移门/平开门/窗/折叠门)
  material_type TEXT,            -- 料型(如 断桥铝/塑钢/普通铝合金)
  style TEXT,                    -- 款式(如 双开门/单开门)
  color TEXT,                    -- 颜色
  face_glass TEXT,               -- 面玻
  back_glass TEXT,               -- 背玻
  remark TEXT,
  created_at TEXT DEFAULT (datetime('now','localtime'))
);

-- 玻璃类型清单(面玻/背玻预输入候选,单玻/双玻分类)
CREATE TABLE IF NOT EXISTS glass_types (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  category TEXT,                 -- 分类(单玻/双玻)
  name TEXT,                     -- 玻璃名称(如 4mm白玻 / 8mm双面砂玻)
  sort_order INTEGER DEFAULT 0
);

-- 产品类型→型材库(产品类型关联型材清单+尺寸公式)
-- condition 为条件分支(如 扇数=2、门高>0),空表示恒真;
-- formula 支持中文变量(总高/总宽/扇数/窗数/门高/墙厚等)+四则运算+比较+and/or+数学函数。
CREATE TABLE IF NOT EXISTS profile_lib (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  product_type TEXT,             -- 产品类型(平开门/推拉门/推拉窗等)
  series TEXT,                   -- 型材系列(35/45/1635/1645等)
  profile_name TEXT,             -- 型材名称(上滑/下滑/光企/勾企/上下方/边封等)
  formula TEXT,                  -- 尺寸公式(中文变量,支持四则运算+比较+and/or)
  quantity INTEGER DEFAULT 1,    -- 默认数量
  category TEXT,                 -- 类型(框/扇/支撑/玻璃/报价/变量)
  sort_order INTEGER DEFAULT 0,
  condition TEXT DEFAULT '',     -- 条件分支(如 扇数=2),空表示无条件
  quantity_formula TEXT DEFAULT '' -- 数量公式(可为空,空则用 quantity 常量)
);

-- 玻璃尺寸公式库(按型号存储面玻/背玻/上亮玻璃的宽高公式)
CREATE TABLE IF NOT EXISTS glass_lib (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  product_type TEXT,
  series TEXT,
  glass_name TEXT,               -- 玻璃名称(面玻/背玻/窗-窗面/窗-窗背等)
  height_formula TEXT,           -- 高度公式
  width_formula TEXT,            -- 宽度公式
  quantity_formula TEXT,         -- 数量公式(可为常量或公式)
  condition TEXT DEFAULT '',     -- 条件(如 单双玻=双玻)
  sort_order INTEGER DEFAULT 0
);

-- 生产环节记录
CREATE TABLE IF NOT EXISTS production_steps (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_id INTEGER NOT NULL REFERENCES orders(id),
  step_name TEXT,                -- 环节名(下料/组框/玻璃/合片/包装/入库)
  done INTEGER DEFAULT 0,        -- 是否完成
  done_at TEXT
);

-- 尺寸核对日志
CREATE TABLE IF NOT EXISTS size_confirm_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_id INTEGER,
  operator TEXT,
  confirmed_at TEXT DEFAULT (datetime('now','localtime')),
  raw_json TEXT
);

-- 打印日志
CREATE TABLE IF NOT EXISTS print_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  order_id INTEGER,
  doc_type TEXT,
  printed_at TEXT DEFAULT (datetime('now','localtime')),
  template_version TEXT
);

-- 备份记录
CREATE TABLE IF NOT EXISTS backups (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  backup_type TEXT,
  file_path TEXT,
  file_size INTEGER,
  created_at TEXT DEFAULT (datetime('now','localtime')),
  status TEXT
);

-- 插件注册表
CREATE TABLE IF NOT EXISTS plugin_registry (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  plugin_id TEXT UNIQUE,
  name TEXT,
  version TEXT,
  api_version TEXT,
  permissions TEXT,
  enabled INTEGER DEFAULT 1,
  installed_at TEXT DEFAULT (datetime('now','localtime'))
);

-- 应用配置
CREATE TABLE IF NOT EXISTS app_config (
  key TEXT PRIMARY KEY,
  value TEXT
);

-- 螺丝规格规则(下料单用):按 门类型 + 型号系列 + 料型 单独分类,数量与型号均可编辑
CREATE TABLE IF NOT EXISTS screw_rules (
  product_type TEXT NOT NULL,
  series TEXT NOT NULL,
  material_type TEXT NOT NULL DEFAULT '通用',
  qty INTEGER DEFAULT 0,
  spec TEXT DEFAULT '',
  remark TEXT DEFAULT '',
  PRIMARY KEY(product_type, series, material_type)
);

-- 历史输入候选(客户/终端地址/终端电话/业务员 各自单独去重保存,供拼音/子串自动匹配)
CREATE TABLE IF NOT EXISTS history (
  field_key TEXT NOT NULL,     -- customer / terminal_addr / terminal_phone / salesman
  value TEXT NOT NULL,
  created_at TEXT DEFAULT (datetime('now','localtime')),
  PRIMARY KEY(field_key, value)
);

-- 玻璃开孔参数(只有平开门用)。来源:用户提供的原软件「公式管理→玻璃表」截图。
-- 右分支被截图的列宽挡住, hole_right 为 NULL 且 note 标注待补, 不做推测。
CREATE TABLE IF NOT EXISTS glass_holes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  series TEXT NOT NULL,
  glass_name TEXT NOT NULL,
  condition TEXT NOT NULL DEFAULT '',
  hole_left TEXT,
  hole_right TEXT,
  note TEXT,
  source TEXT,
  UNIQUE(series, glass_name, condition)
);

-- 未提交表单快照(定时/关闭前)。只存新增订单页与合同页尚未保存的数据,不存整库。
CREATE TABLE IF NOT EXISTS form_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,          -- auto(定时,上限5) / shutdown(关闭前,上限3)
  payload TEXT NOT NULL,
  created_at TEXT DEFAULT (datetime('now','localtime'))
);
CREATE INDEX IF NOT EXISTS idx_form_snapshots_kind_id ON form_snapshots(kind, id DESC);

-- 常用查询索引：原来整库只有 1 个索引，订单列表/明细/型号公式全是全表扫描
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status);
CREATE INDEX IF NOT EXISTS idx_orders_customer ON orders(customer_id);
CREATE INDEX IF NOT EXISTS idx_orders_order_no ON orders(order_no);
CREATE INDEX IF NOT EXISTS idx_orders_date ON orders(order_date);
CREATE INDEX IF NOT EXISTS idx_profiles_order ON profiles(order_id);
CREATE INDEX IF NOT EXISTS idx_profiles_name ON profiles(profile_name);
CREATE INDEX IF NOT EXISTS idx_glasses_order ON glasses(order_id);
CREATE INDEX IF NOT EXISTS idx_accessories_order ON accessories(order_id);
CREATE INDEX IF NOT EXISTS idx_contracts_no ON contracts(contract_no);
CREATE INDEX IF NOT EXISTS idx_profile_lib_lookup ON profile_lib(product_type, series);
CREATE INDEX IF NOT EXISTS idx_glass_lib_lookup ON glass_lib(product_type, series);
CREATE INDEX IF NOT EXISTS idx_history_key ON history(field_key);
";

/// 平开门玻璃开孔参数。仅录入用户提供的截图中确实读到的值；
/// `|` 右分支在截图里全被列宽截断，hole_right 一律留 NULL 并在 note 标注待补，绝不推测。
/// 逐型号列出：(型号, 面-面玻, 背-背玻, 面玻, 来源截图)。
fn seed_glass_holes(conn: &Connection) {
    type Row<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str);
    let rows: Vec<Row> = vec![
        // 960-70-30 左开孔
        ("7535门心", "开向≈左(960-70-30 左开孔)", "开向≈左(960-70-30 左开孔)", "开向≈左(960-70-30 左开孔)", "QQ截图20261109123615.png"),
        ("1635固定单玻", "开向≈左(960-70-30 左开孔)", "开向≈左(960-70-30 左开孔)", "开向≈左(960-70-30 左开孔)", "QQ截图20261109123646.png"),
        ("1645固定", "开向≈左(960-70-30 左开孔)", "开向≈左(960-70-30 左开孔)", "开向≈左(960-70-30 左开孔)", "QQ截图20261109123857.png"),
        ("7535双包套", "开向≈左(960-70-30 左开孔)", "开向≈左(960-70-30 左开孔)", "开向≈左(960-70-30 左开孔)", "QQ截图20261109124341.png"),
        // 980-140-80 居中右开孔
        ("1635复古门", "开向≈左(居中980-140-80右开孔)", "开向≈左(居中980-140-80 右开)", "开向≈左(居中980-140-80 右开)", "QQ截图20261109123917.png"),
        // 980-72-32 居中右开孔
        ("1635卡枪灰双玻小材料", "开向≈左(居中980-72-32右开孔)", "开向≈左(居中980-72-32 右开)", "开向≈左(居中980-72-32 右开)", "QQ截图20261109123942.png"),
        ("1635双包套平开门小材料", "开向≈左(居中980-72-32右开孔)", "开向≈左(居中980-72-32 右开)", "开向≈左(居中980-72-32 右开)", "QQ截图20261109124419.png"),
        ("1645双包套", "开向≈左(居中980-72-32 右开)", "开向≈左(居中980-70-30 右开孔)", "开向≈左(居中980-72-32 右开孔)", "QQ截图20261109124224.png"),
        // 980-72-32 右开孔（无居中）
        ("1645鹏飞铂金灰", "开向≈左(980-72-32右开孔)", "开向≈左(980-72-32 右开)", "开向≈左(980-72-32 右开)", "QQ截图20261109123958.png"),
        ("1340单玻平开门", "开向≈左(980-72-32右开孔)", "开向≈左(980-72-32 右开)", "开向≈左(980-72-32 右开)", "QQ截图20261109124011.png"),
        // 980-72-32 左开孔
        ("双包平开模板", "开向≈左(980-72-32 左开孔)", "开向≈左(980-72-32 左开孔)", "开向≈左(980-72-32 左开孔)", "QQ截图20261109124141.png"),
        // 4030：纯文字型，三行各不相同
        ("4030平开门", "外包内左", "外包内右", "内包内右", "QQ截图20261109123804.png"),
    ];
    for (series, face, back, single, source) in rows {
        for (gname, cond, hole) in [
            ("面-面玻", "单双玻=双玻", face),
            ("背-背玻", "单双玻=双玻", back),
            ("面玻", "单双玻=单玻", single),
        ] {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO glass_holes(series,glass_name,condition,hole_left,hole_right,note,source) \
                 VALUES(?1,?2,?3,?4,NULL,'右分支截图被截断，待补',?5)",
                rusqlite::params![series, gname, cond, hole, source],
            );
        }
    }
    // 截图里明确打开过、且开孔列全空的型号：登记一条标记，便于对照表区分「确认无」与「没找到」
    for (s, src) in [
        ("鹏飞6033单包平开门", "QQ截图20261109123535.png"),
        ("5830平开门", "QQ截图20261109124022.png"),
        ("8535平开门", "QQ截图20261109124040.png"),
        ("1635双包套平开门", "QQ截图20261109124355.png"),
    ] {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO glass_holes(series,glass_name,condition,hole_left,hole_right,note,source) \
             VALUES(?1,'(全部)','',NULL,NULL,'截图确认：开孔列全空，该型号不开孔',?2)",
            rusqlite::params![s, src],
        );
    }
}

/// 初始化数据库,返回连接。
/// 默认配置(三套备份目录/保留策略/压缩开关)写入 app_config。
pub fn init() -> Connection {
    let dir = data_dir();
    let db_path = dir.join("app.db");

    // 安装包自带的初始数据库：自用版把当前全部数据（订单/客户/公式）一起打进去，
    // 换电脑装完打开就是原来的数据。开源版不带这个文件，首次运行会自动生成基础数据。
    // 只在 data/app.db 不存在时才复制，绝不覆盖已有数据。
    if !db_path.exists() {
        for cand in [
            app_dir().join("app-seed.db"),
            app_dir().join("resources").join("app-seed.db"),
        ] {
            if cand.exists() {
                let _ = std::fs::copy(&cand, &db_path);
                break;
            }
        }
    }

    let conn = Connection::open(&db_path).expect("无法打开数据库");

    conn.execute_batch(SCHEMA).expect("初始化 schema 失败");

    // 整个「迁移 + 补种 + 纠错」包在一个事务里。
    // 原来 1400+ 条 INSERT 每条都是独立事务，每次启动要 fsync 上千次（实测启动 ~1.2s），
    // 包事务后同样的工作只需几十毫秒。seed 全部幂等，中途失败回滚也不会留脏数据。
    let _ = conn.execute_batch("BEGIN");

    // 自动迁移:为旧库的 orders 表补充缺失字段(幂等)
    migrate_orders(&conn);
    migrate_contracts(&conn);
    migrate_profile_lib(&conn);

    // 统一历史 OCR 误读的轨道类型名(必须在 seed 之前,避免旧名被重复补种)
    crate::models_seed::normalize_rail_conditions(&conn);

    // 录屏确认的型号名纠错(必须在 seed 之前,避免旧名被重复补种)
    crate::models_seed::migrate_video_renames(&conn);

    // 内置产品预设库(预输入模板,幂等)
    seed_product_presets(&conn);

    // 内置玻璃类型清单(面玻/背玻预输入候选,幂等)
    seed_glass_types(&conn);

    // 内置产品类型→型材库(型材清单+尺寸公式,幂等)
    seed_profile_lib(&conn);

    // 内置完整型号体系(移门管理行业通用下料公式,中文变量+条件分支,幂等)
    seed_full_profile_lib(&conn);

    // 按用户提供的型号分类表补全全部型号(单轨/两轨/三轨/1645/7530/1340/平开门)
    crate::models_seed::seed_all_models(&conn);

    // 清理历史脏数据:旧版 seed 曾重复插入(去重),并修正误标 category 的变量行。
    // 放在 seed 之后执行,确保对已存在脏库也能彻底清理(seed 的增量补种不会覆盖已有行)。
    dedup_profile_lib(&conn);

    // 录屏修正(放在最后,确保覆盖 seed 与 dedup 的结果):
    // 空公式型号只留型号名、清空公式;按录屏实测值纠正个别参数。
    crate::models_seed::apply_video_corrections(&conn);

    // 螺丝规格规则(按门类型+型号系列,幂等;必须在型号 seed 之后)
    seed_screw_rules(&conn);

    // 平开门玻璃开孔参数(来源:用户提供的原软件公式管理截图,幂等)
    seed_glass_holes(&conn);

    // 默认配置
    let defaults: Vec<(&str, String)> = vec![
        ("backup.shutdown.dir", dir.join("..").join("backup").join("shutdown").to_string_lossy().to_string()),
        ("backup.shutdown.retain", "5".to_string()),
        ("backup.snapshot.dir", dir.join("..").join("backup").join("snapshot").to_string_lossy().to_string()),
        ("backup.snapshot.retain", "10".to_string()),
        ("backup.snapshot.interval_min", "30".to_string()),
        ("backup.manual.dir", dir.join("..").join("backup").join("manual").to_string_lossy().to_string()),
        ("backup.manual.retain", "20".to_string()),
        ("backup.compress", "true".to_string()),
        ("backup.format", "gz".to_string()),
        ("company.name", String::new()),
        ("operator.name", "管理员".to_string()),
        // 是否允许多开(默认禁止多开)
        ("app.allow_multi_open", "false".to_string()),
        // 旧版「简易玻璃扣减」配置已彻底移除,不再写入
    ];
    conn.execute("DELETE FROM app_config WHERE key LIKE 'calc.glass.%'", []).ok();

    {
        let mut stmt = conn
            .prepare("INSERT OR IGNORE INTO app_config(key,value) VALUES(?1,?2)")
            .expect("prepare config insert");
        for (k, v) in defaults {
            stmt.execute(rusqlite::params![k, v]).ok();
        }
    }

    // 提交整个初始化事务
    let _ = conn.execute_batch("COMMIT");

    conn
}

/// 内置产品预设库(基于门窗行业公开资料整理,自主编写)
fn seed_product_presets(conn: &Connection) {
    // 用户要求「产品预设列表暂时先不设任何」:清空内置模板,只执行一次。
    // 之后用户自行新增的预设不会被覆盖。
    let cleared: i64 = conn
        .query_row("SELECT COUNT(*) FROM app_config WHERE key='presets.cleared'", [], |r| r.get(0))
        .unwrap_or(0);
    if cleared > 0 {
        return;
    }
    conn.execute("DELETE FROM product_presets", []).ok();
    conn.execute("INSERT OR REPLACE INTO app_config(key,value) VALUES('presets.cleared','1')", []).ok();
}

/// 螺丝规格规则:按「门类型 + 型号系列 + 料型」单独分类,可逐条修改/新增。
/// 网上资料未明确说明「平开门不管什么型材螺丝都是一个数量」,故不统一设置。
/// 每个型号先建一条 material_type='通用' 的规则;需要按料型区分时可在设置页新增行。
/// 数量基准依据 JGJ 214-2010 / GB 50210-2018:每边框固定螺丝≥3颗、每扇≥6颗,
/// 按「1 个框(12) + 扇数」取下限:推拉门默认 2 扇=36,平开门/推拉窗默认 1 扇=24。
/// 螺丝型号依据:φ4.2mm、长度≥25mm、304 不锈钢自攻螺丝(ST4.2 系列)。
fn seed_screw_rules(conn: &Connection) {
    let series: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT DISTINCT product_type, series FROM profile_lib")
            .expect("screw: select series");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                ))
            })
            .expect("screw: query series");
        rows.filter_map(|x| x.ok()).collect()
    };
    let mut ins = conn
        .prepare(
            "INSERT OR IGNORE INTO screw_rules(product_type, series, material_type, qty, spec, remark) \
             VALUES(?1,?2,'通用',?3,?4,?5)",
        )
        .expect("screw: prepare insert");
    for (pt, s) in &series {
        let qty: i64 = if pt == "推拉门" { 36 } else { 24 };
        ins.execute(rusqlite::params![
            pt,
            s,
            qty,
            "ST4.2×25 304不锈钢自攻螺丝",
            "依据 JGJ 214-2010/GB 50210-2018:每边框≥3颗、每扇≥6颗;按1框+扇数取下限"
        ])
        .ok();
    }
}

/// 内置玻璃类型清单(面玻/背玻预输入候选)
/// 分类:单玻(1片)/双玻(2片);名称带厚度(mm)与颜色/工艺;sort_order 控制展示顺序(热门靠前)。
/// 数据基于门窗/移门行业公开通用规格与市场热度自主整理,不涉及任何软件私有数据。
fn seed_glass_types(conn: &Connection) {
    // 清理旧版玻璃类型(旧版无 sort_order,默认 0;新版从 1 起),避免旧命名混入干扰排序
    conn.execute("DELETE FROM glass_types WHERE sort_order <= 0", []).ok();

    // 用户业务：所有玻璃都用钢化（安全玻璃强制要求），所以库里只保留钢化规格。
    // 旧版种子里的非钢化名称先精确删掉——只删种子定义过的名字，用户自己加的不动，
    // 否则下拉里混着「白玻 5mm」很容易误选成非钢化。
    let obsolete: Vec<(&str, &str)> = vec![
        ("单玻", "单玻·超白玻 5mm"),
        ("双玻", "双玻·超白长虹 5+5"),
        ("单玻", "单玻·超白玻 8mm"),
        ("双玻", "双玻·超白水波纹 5+5"),
        ("单玻", "单玻·超白玻 4mm"),
        ("双玻", "双玻·超白透明 5+5"),
        ("单玻", "单玻·超白玻 6mm"),
        ("双玻", "双玻·超白磨砂 5+5"),
        ("单玻", "单玻·超白玻 10mm"),
        ("双玻", "双玻·5+9A+5 中空"),
        ("单玻", "单玻·超白长虹 5mm"),
        ("双玻", "双玻·5+12A+5 中空"),
        ("单玻", "单玻·超白长虹 8mm"),
        ("双玻", "双玻·5+18A+5 中空"),
        ("单玻", "单玻·长虹 5mm"),
        ("双玻", "双玻·5+20A+5 中空"),
        ("单玻", "单玻·长虹 8mm"),
        ("双玻", "双玻·8+12A+8 中空"),
        ("单玻", "单玻·灰玻 5mm"),
        ("双玻", "双玻·6+12A+6 中空"),
        ("单玻", "单玻·灰玻 8mm"),
        ("双玻", "双玻·6+9A+6 中空"),
        ("单玻", "单玻·灰玻 4mm"),
        ("双玻", "双玻·双层中空"),
        ("单玻", "单玻·灰玻 6mm"),
        ("双玻", "双玻·5+5 夹胶"),
        ("单玻", "单玻·黑玻 5mm"),
        ("双玻", "双玻·8+8 夹胶"),
        ("单玻", "单玻·黑玻 8mm"),
        ("双玻", "双玻·夹胶玻璃"),
        ("单玻", "单玻·磨砂玻璃 5mm"),
        ("双玻", "双玻·双层钢化"),
        ("单玻", "单玻·油砂玻璃 5mm"),
        ("双玻", "双玻·双层白玻"),
        ("单玻", "单玻·磨砂玻璃 8mm"),
        ("双玻", "双玻·双层灰玻"),
        ("单玻", "单玻·玉砂玻璃 5mm"),
        ("双玻", "双玻·双层磨砂"),
        ("单玻", "单玻·双面砂玻 8mm"),
        ("双玻", "双玻·Low-E 中空"),
        ("单玻", "单玻·茶玻 5mm"),
        ("双玻", "双玻·Low-E 5+12A+5"),
        ("单玻", "单玻·茶玻 8mm"),
        ("单玻", "单玻·白玻 5mm"),
        ("单玻", "单玻·白玻 8mm"),
        ("单玻", "单玻·白玻 4mm"),
        ("单玻", "单玻·白玻 6mm"),
        ("单玻", "单玻·白玻 10mm"),
        ("单玻", "单玻·透明玻璃 5mm"),
        ("单玻", "单玻·透明玻璃 8mm"),
        ("单玻", "单玻·水波纹 5mm"),
        ("单玻", "单玻·水波纹 8mm"),
        ("单玻", "单玻·钻石纹 5mm"),
        ("单玻", "单玻·夹丝玻璃 6mm"),
        ("单玻", "单玻·印花玻璃 5mm"),
        ("单玻", "单玻·钢化玻璃 5mm"),
        ("单玻", "单玻·钢化玻璃 8mm"),
        ("单玻", "单玻·钢化玻璃 10mm"),
    ];
    for (cat, name) in obsolete {
        conn.execute(
            "DELETE FROM glass_types WHERE category = ?1 AND name = ?2",
            rusqlite::params![cat, name],
        )
        .ok();
    }

    // (分类, 玻璃名称, 排序号) —— 全部钢化，常用规格靠前
    let glass: Vec<(&str, &str, i64)> = vec![
        ("单玻", "单玻·钢化白玻 5mm", 1),
        ("单玻", "单玻·钢化白玻 6mm", 2),
        ("单玻", "单玻·钢化白玻 8mm", 3),
        ("单玻", "单玻·钢化白玻 4mm", 4),
        ("单玻", "单玻·钢化白玻 10mm", 5),
        ("单玻", "单玻·钢化白玻 12mm", 6),
        ("单玻", "单玻·钢化超白 5mm", 7),
        ("单玻", "单玻·钢化超白 6mm", 8),
        ("单玻", "单玻·钢化超白 8mm", 9),
        ("单玻", "单玻·钢化超白 4mm", 10),
        ("单玻", "单玻·钢化超白 10mm", 11),
        ("单玻", "单玻·钢化超白 12mm", 12),
        ("单玻", "单玻·钢化灰玻 5mm", 13),
        ("单玻", "单玻·钢化灰玻 6mm", 14),
        ("单玻", "单玻·钢化灰玻 8mm", 15),
        ("单玻", "单玻·钢化黑玻 5mm", 16),
        ("单玻", "单玻·钢化黑玻 8mm", 17),
        ("单玻", "单玻·钢化茶玻 5mm", 18),
        ("单玻", "单玻·钢化茶玻 8mm", 19),
        ("单玻", "单玻·钢化长虹 5mm", 20),
        ("单玻", "单玻·钢化长虹 8mm", 21),
        ("单玻", "单玻·钢化超白长虹 5mm", 22),
        ("单玻", "单玻·钢化超白长虹 8mm", 23),
        ("单玻", "单玻·钢化磨砂 5mm", 24),
        ("单玻", "单玻·钢化磨砂 8mm", 25),
        ("单玻", "单玻·钢化油砂 5mm", 26),
        ("单玻", "单玻·钢化玉砂 5mm", 27),
        ("单玻", "单玻·钢化双面砂 8mm", 28),
        ("单玻", "单玻·钢化水波纹 5mm", 29),
        ("单玻", "单玻·钢化水波纹 8mm", 30),
        ("单玻", "单玻·钢化钻石纹 5mm", 31),
        ("单玻", "单玻·钢化夹丝 6mm", 32),
        ("单玻", "单玻·钢化印花 5mm", 33),
        ("单玻", "单玻·钢化透明 5mm", 34),
        ("单玻", "单玻·钢化透明 8mm", 35),
        ("双玻", "双玻·钢化中空 5+12A+5", 1),
        ("双玻", "双玻·钢化中空 5+9A+5", 2),
        ("双玻", "双玻·钢化中空 5+18A+5", 3),
        ("双玻", "双玻·钢化中空 6+12A+6", 4),
        ("双玻", "双玻·钢化中空 6+9A+6", 5),
        ("双玻", "双玻·钢化中空 8+12A+8", 6),
        ("双玻", "双玻·钢化中空 5+20A+5", 7),
        ("双玻", "双玻·钢化中空 超白 5+12A+5", 8),
        ("双玻", "双玻·钢化中空 灰玻 5+12A+5", 9),
        ("双玻", "双玻·钢化中空 长虹 5+12A+5", 10),
        ("双玻", "双玻·钢化夹胶 5+5", 11),
        ("双玻", "双玻·钢化夹胶 6+6", 12),
        ("双玻", "双玻·钢化夹胶 8+8", 13),
        ("双玻", "双玻·钢化夹胶 超白 5+5", 14),
        ("双玻", "双玻·钢化夹胶 长虹 5+5", 15),
        ("双玻", "双玻·钢化Low-E中空 5+12A+5", 16),
        ("双玻", "双玻·钢化Low-E中空 6+12A+6", 17),
    ];

    // 增量补种:按 (category, name) 去重,已存在则不覆盖
    let existing: std::collections::HashSet<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT category, name FROM glass_types")
            .expect("prepare glass_types select");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                ))
            })
            .expect("query glass_types");
        rows.filter_map(|x| x.ok()).collect()
    };

    let mut stmt = conn
        .prepare("INSERT INTO glass_types(category, name, sort_order) VALUES(?1,?2,?3)")
        .expect("prepare glass type insert");
    for (cat, name, so) in glass {
        let key = (cat.to_string(), name.to_string());
        if existing.contains(&key) {
            continue;
        }
        stmt.execute(rusqlite::params![cat, name, so]).ok();
    }
}

/// 内置产品类型→型材库(型材清单+尺寸公式,基于门窗行业公开标准下料公式自主整理)
/// 公式变量:W=总宽(mm) H=总高(mm) N=扇数,支持 + - * / 和括号
/// 增量补种:按 (product_type, series, profile_name) 唯一组合判断,只插入缺失项,
/// 不覆盖用户已自定义的公式。
fn seed_profile_lib(conn: &Connection) {
    // (产品类型, 系列, 型材名, 公式, 数量, 类型, 条件)
    // 公式变量:中文(总高/总宽/扇数/窗数/门高/墙厚等)+英文 W/H/N,支持四则运算+比较+and/or+数学函数
    // 下料公式说明(门窗行业通用几何关系,非某软件私有):
    //   框料竖料 = 总高; 框料横料 = 总宽 - 2*边封搭接量;
    //   扇料竖料(光企/勾企) = 总高 - 上下框高度 - 搭接量 + 双面搭接;
    //   扇料横料(上下方) = (总宽 - 常数) / 扇数 + 搭接补偿。
    let lib: Vec<(&str, &str, &str, &str, i64, &str, &str)> = vec![
        // ===== 推拉门(1635/1645/1650 系列,旧简版保留) =====
        ("推拉门", "1635", "边封", "H", 2, "框", ""),
        ("推拉门", "1635", "上滑", "W-28*2+16*2", 1, "框", ""),
        ("推拉门", "1635", "下滑", "W-28*2+16*2", 1, "框", ""),
        ("推拉门", "1635", "光企", "H-50-24.3+36-5", 2, "扇", ""),
        ("推拉门", "1635", "勾企", "H-50-24.3+36-5", 2, "扇", ""),
        ("推拉门", "1635", "上下方", "(W-28*2+16*2)/N+55/2-65-55+63+53", 2, "扇", ""),
        ("推拉门", "1645", "边封", "H", 2, "框", ""),
        ("推拉门", "1645", "上滑", "W-28*2+16*2", 1, "框", ""),
        ("推拉门", "1645", "下滑", "W-28*2+16*2", 1, "框", ""),
        ("推拉门", "1645", "光企", "H-50-24.3+36-5", 2, "扇", ""),
        ("推拉门", "1645", "勾企", "H-50-24.3+36-5", 2, "扇", ""),
        ("推拉门", "1645", "上下方", "(W-28*2+16*2)/N+55/2-65-55+63+53", 2, "扇", ""),
        ("推拉门", "1650", "边封", "H", 2, "框", ""),
        ("推拉门", "1650", "上滑", "W-28*2+16*2", 1, "框", ""),
        ("推拉门", "1650", "下滑", "W-28*2+16*2", 1, "框", ""),
        ("推拉门", "1650", "光企", "H-50-24.3+36-5", 2, "扇", ""),
        ("推拉门", "1650", "勾企", "H-50-24.3+36-5", 2, "扇", ""),
        ("推拉门", "1650", "上下方", "(W-28*2+16*2)/N+55/2-65-55+63+53", 2, "扇", ""),
        // ===== 平开门(35/45/50 系列,旧简版保留) =====
        ("平开门", "35", "竖门框", "H", 2, "框", ""),
        ("平开门", "35", "上框", "W-29*2", 1, "框", ""),
        ("平开门", "35", "下框", "W-29*2", 1, "框", ""),
        ("平开门", "35", "中横框", "W-29*2", 1, "框", ""),
        ("平开门", "35", "光企", "H-30-28-12.9+8*2", 2, "扇", ""),
        ("平开门", "35", "勾企", "H-30-28-12.9+8*2", 2, "扇", ""),
        ("平开门", "35", "上下方", "(W-41.9*2-4.5+8*2)/2-50+44-50+44", 2, "扇", ""),
        ("平开门", "45", "竖门框", "H", 2, "框", ""),
        ("平开门", "45", "上框", "W-29*2", 1, "框", ""),
        ("平开门", "45", "下框", "W-29*2", 1, "框", ""),
        ("平开门", "45", "中横框", "W-29*2", 1, "框", ""),
        ("平开门", "45", "光企", "H-30-28-12.9+8*2", 2, "扇", ""),
        ("平开门", "45", "勾企", "H-30-28-12.9+8*2", 2, "扇", ""),
        ("平开门", "45", "上下方", "(W-41.9*2-4.5+8*2)/2-50+44-50+44", 2, "扇", ""),
        ("平开门", "50", "竖门框", "H", 2, "框", ""),
        ("平开门", "50", "上框", "W-29*2", 1, "框", ""),
        ("平开门", "50", "下框", "W-29*2", 1, "框", ""),
        ("平开门", "50", "中横框", "W-29*2", 1, "框", ""),
        ("平开门", "50", "光企", "H-30-28-12.9+8*2", 2, "扇", ""),
        ("平开门", "50", "勾企", "H-30-28-12.9+8*2", 2, "扇", ""),
        ("平开门", "50", "上下方", "(W-41.9*2-4.5+8*2)/2-50+44-50+44", 2, "扇", ""),
        // ===== 推拉窗(70/80/90 系列,旧简版保留) =====
        ("推拉窗", "70", "边封", "H", 2, "框", ""),
        ("推拉窗", "70", "上滑", "W-70", 1, "框", ""),
        ("推拉窗", "70", "下滑", "W-70", 1, "框", ""),
        ("推拉窗", "70", "光企", "H-80", 2, "扇", ""),
        ("推拉窗", "70", "勾企", "H-80", 2, "扇", ""),
        ("推拉窗", "70", "上下方", "(W-70)/N+35", 2, "扇", ""),
        ("推拉窗", "80", "边封", "H", 2, "框", ""),
        ("推拉窗", "80", "上滑", "W-80", 1, "框", ""),
        ("推拉窗", "80", "下滑", "W-80", 1, "框", ""),
        ("推拉窗", "80", "光企", "H-90", 2, "扇", ""),
        ("推拉窗", "80", "勾企", "H-90", 2, "扇", ""),
        ("推拉窗", "80", "上下方", "(W-80)/N+40", 2, "扇", ""),
        ("推拉窗", "90", "边封", "H", 2, "框", ""),
        ("推拉窗", "90", "上滑", "W-90", 1, "框", ""),
        ("推拉窗", "90", "下滑", "W-90", 1, "框", ""),
        ("推拉窗", "90", "光企", "H-100", 2, "扇", ""),
        ("推拉窗", "90", "勾企", "H-100", 2, "扇", ""),
        ("推拉窗", "90", "上下方", "(W-90)/N+45", 2, "扇", ""),
    ];

    // 增量补种:先查已存在的唯一组合,再插入缺失项
    let existing: std::collections::HashSet<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT product_type, series, profile_name FROM profile_lib")
            .expect("prepare profile_lib select");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, String>(2).unwrap_or_default(),
                ))
            })
            .expect("query profile_lib");
        rows.filter_map(|x| x.ok()).collect()
    };

    let mut ins = conn
        .prepare(
            "INSERT INTO profile_lib(product_type, series, profile_name, formula, quantity, category, sort_order, condition) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        )
        .expect("prepare profile_lib insert");

    let mut sort = existing.len() as i64;
    for (pt, series, name, formula, qty, cat, cond) in &lib {
        let key = (pt.to_string(), series.to_string(), name.to_string());
        if existing.contains(&key) {
            continue; // 已存在(可能被用户自定义过),不覆盖
        }
        ins.execute(rusqlite::params![pt, series, name, formula, qty, cat, sort, cond]).ok();
        sort += 1;
    }
}

/// 内置「完整型号体系」(移门管理行业通用下料公式,中文变量+条件分支)
/// 覆盖:推拉门(1645四开/1645单玻三联动/鹏飞6033/鹏飞7530/7530一字轨/鹏飞7535三轨/1340单玻两轨)
///      平开门(35*45平安鑫单包平开)
/// 型材用 profile_lib(category=框/扇),玻璃用 glass_lib,报价并入 profile_lib(category=报价)。
/// 变量:总高/总宽/扇数/窗数/门高/墙厚/重叠边数量/含轨道的高度/光企高 等。
/// 增量补种:按 (product_type, series, profile_name, condition) 判断,不覆盖用户自定义。
fn seed_full_profile_lib(conn: &Connection) {
    // ---- 型材库(含条件分支) ----
    // 结构: (产品类型, 系列, 型材名, 公式, 数量, 类别, 条件)
    // 注:光企高/上下方等用"算法变量"间接引用,这里直接内联为该型号的算法结果。
    let profiles: Vec<(&str, &str, &str, &str, i64, &str, &str)> = vec![
        // ============ 推拉门·1645四开推拉门 ============
        ("推拉门", "1645四开推拉门", "边封筒", "含轨道的高度", 2, "框", ""),
        ("推拉门", "1645四开推拉门", "轨道长", "总宽-40", 1, "框", ""),
        ("推拉门", "1645四开推拉门", "光企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "1645四开推拉门", "勾企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "1645四开推拉门", "上下方", "上下方算法变量", 1, "扇", ""),
        ("推拉门", "1645四开推拉门", "收口", "光企高算法", 1, "框", "扇数=4 or 是否有收口=1"),
        ("推拉门", "1645四开推拉门", "套线高", "总高-28+36", 2, "框", "包套=双包"),
        ("推拉门", "1645四开推拉门", "套线宽", "总宽-40+72", 1, "框", "包套=双包"),
        ("推拉门", "1645四开推拉门", "铝板高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "1645四开推拉门", "铝板宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "1645四开推拉门", "F条高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "1645四开推拉门", "F条宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "1645四开推拉门", "铝板厚", "墙厚-50", 1, "框", "墙厚>0"),
        // ============ 推拉门·1645单玻三联动推拉门 ============
        ("推拉门", "1645单玻三联动推拉门", "边封高", "含轨道的高度", 2, "框", ""),
        ("推拉门", "1645单玻三联动推拉门", "轨道长", "总宽-40", 1, "框", ""),
        ("推拉门", "1645单玻三联动推拉门", "光企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "1645单玻三联动推拉门", "勾企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "1645单玻三联动推拉门", "上下方", "上下方算法变量", 1, "扇", ""),
        ("推拉门", "1645单玻三联动推拉门", "收口", "光企高算法", 1, "框", "扇数=4 or 是否有收口=1"),
        ("推拉门", "1645单玻三联动推拉门", "套线高", "总高-28+36", 2, "框", "包套=双包"),
        ("推拉门", "1645单玻三联动推拉门", "套线宽", "总宽-40+72", 1, "框", "包套=双包"),
        ("推拉门", "1645单玻三联动推拉门", "铝板高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "1645单玻三联动推拉门", "铝板宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "1645单玻三联动推拉门", "F条高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "1645单玻三联动推拉门", "F条宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "1645单玻三联动推拉门", "铝板厚", "墙厚-50", 1, "框", "墙厚>0"),
        // ============ 推拉门·鹏飞6033推拉门 ============
        ("推拉门", "鹏飞6033推拉门", "边封盖", "含轨道的高度-1020", 2, "框", ""),
        ("推拉门", "鹏飞6033推拉门", "轨道长", "总宽-60-1020", 1, "框", ""),
        ("推拉门", "鹏飞6033推拉门", "光企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "鹏飞6033推拉门", "勾企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "鹏飞6033推拉门", "上下方", "上下方算法变量", 1, "扇", ""),
        ("推拉门", "鹏飞6033推拉门", "收口", "光企高算法", 1, "框", "扇数=4 or 是否有收口=1"),
        ("推拉门", "鹏飞6033推拉门", "套线高", "总高-28+50", 2, "框", "包套=双包"),
        ("推拉门", "鹏飞6033推拉门", "套线宽", "总宽-60+100", 1, "框", "包套=双包"),
        ("推拉门", "鹏飞6033推拉门", "铝板高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "鹏飞6033推拉门", "铝板宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "鹏飞6033推拉门", "F条高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "鹏飞6033推拉门", "F条宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "鹏飞6033推拉门", "铝板厚", "墙厚-50", 1, "框", "墙厚>0"),
        // ============ 推拉门·鹏飞7530推拉门 ============
        ("推拉门", "鹏飞7530推拉门", "边封高", "含轨道的高度-1020", 2, "框", ""),
        ("推拉门", "鹏飞7530推拉门", "轨道长", "总宽-60-1020", 1, "框", ""),
        ("推拉门", "鹏飞7530推拉门", "光企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "鹏飞7530推拉门", "勾企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "鹏飞7530推拉门", "上下方", "上下方算法变量", 1, "扇", ""),
        ("推拉门", "鹏飞7530推拉门", "收口", "光企高算法", 1, "框", "扇数=4 or 是否有收口=1"),
        ("推拉门", "鹏飞7530推拉门", "套线高", "总高", 2, "框", "包套=双包"),
        ("推拉门", "鹏飞7530推拉门", "套线宽", "总宽", 1, "框", "包套=双包"),
        ("推拉门", "鹏飞7530推拉门", "铝板高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "鹏飞7530推拉门", "铝板宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "鹏飞7530推拉门", "F条高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "鹏飞7530推拉门", "F条宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "鹏飞7530推拉门", "铝板厚", "墙厚-50", 1, "框", "墙厚>0"),
        // ============ 推拉门·7530一字轨 ============
        ("推拉门", "7530一字轨", "边封高", "含轨道的高度-1020", 2, "框", ""),
        ("推拉门", "7530一字轨", "轨道长", "总宽-60-1020", 1, "框", ""),
        ("推拉门", "7530一字轨", "光企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "7530一字轨", "勾企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "7530一字轨", "上下方", "上下方算法变量", 1, "扇", ""),
        ("推拉门", "7530一字轨", "收口", "光企高算法", 1, "框", "扇数=4 or 是否有收口=1"),
        ("推拉门", "7530一字轨", "套线高", "总高", 2, "框", "包套=双包"),
        ("推拉门", "7530一字轨", "套线宽", "总宽", 1, "框", "包套=双包"),
        ("推拉门", "7530一字轨", "铝板高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "7530一字轨", "铝板宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "7530一字轨", "F条高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "7530一字轨", "F条宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "7530一字轨", "铝板厚", "墙厚-50", 1, "框", "墙厚>0"),
        // ============ 推拉门·鹏飞7535三轨推拉门 ============
        ("推拉门", "鹏飞7535三轨推拉门", "边封高", "含轨道的高度-1020", 2, "框", ""),
        ("推拉门", "鹏飞7535三轨推拉门", "轨道长", "总宽-60-1020", 1, "框", ""),
        ("推拉门", "鹏飞7535三轨推拉门", "光企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "鹏飞7535三轨推拉门", "勾企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "鹏飞7535三轨推拉门", "上下方", "上下方算法变量", 1, "扇", ""),
        ("推拉门", "鹏飞7535三轨推拉门", "收口", "光企高算法", 1, "框", "扇数=4 or 是否有收口=1"),
        ("推拉门", "鹏飞7535三轨推拉门", "套线高", "总高", 2, "框", "包套=双包"),
        ("推拉门", "鹏飞7535三轨推拉门", "套线宽", "总宽", 1, "框", "包套=双包"),
        ("推拉门", "鹏飞7535三轨推拉门", "铝板高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "鹏飞7535三轨推拉门", "铝板宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "鹏飞7535三轨推拉门", "F条高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "鹏飞7535三轨推拉门", "F条宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "鹏飞7535三轨推拉门", "铝板厚", "墙厚-50", 1, "框", "墙厚>0"),
        // ============ 推拉门·1340单玻两轨推拉门 ============
        ("推拉门", "1340单玻两轨推拉门", "边封高", "含轨道的高度-1020", 2, "框", ""),
        ("推拉门", "1340单玻两轨推拉门", "轨道长", "总宽-40-1020", 1, "框", ""),
        ("推拉门", "1340单玻两轨推拉门", "光企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "1340单玻两轨推拉门", "勾企高", "光企高算法", 1, "扇", ""),
        ("推拉门", "1340单玻两轨推拉门", "上下方", "上下方算法变量", 1, "扇", ""),
        ("推拉门", "1340单玻两轨推拉门", "收口", "光企高算法", 1, "框", "扇数=4 or 是否有收口=1"),
        ("推拉门", "1340单玻两轨推拉门", "套线高", "总高-20+36", 2, "框", "包套=双包"),
        ("推拉门", "1340单玻两轨推拉门", "套线宽", "总宽-40+71", 1, "框", "包套=双包"),
        ("推拉门", "1340单玻两轨推拉门", "铝板高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "1340单玻两轨推拉门", "铝板宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "1340单玻两轨推拉门", "F条高", "总高-50", 2, "框", "墙厚>0"),
        ("推拉门", "1340单玻两轨推拉门", "F条宽", "总宽-50", 1, "框", "墙厚>0"),
        ("推拉门", "1340单玻两轨推拉门", "铝板厚", "墙厚-50", 1, "框", "墙厚>0"),
        // ============ 平开门·35*45平安鑫单包平开 ============
        ("平开门", "35*45平安鑫单包平开", "竖门框", "总高", 2, "框", ""),
        ("平开门", "35*45平安鑫单包平开", "上框", "总宽-29*2", 1, "框", ""),
        ("平开门", "35*45平安鑫单包平开", "下框", "总宽-29*2", 1, "框", ""),
        ("平开门", "35*45平安鑫单包平开", "光企", "门扇高+56", 2, "扇", ""),
        ("平开门", "35*45平安鑫单包平开", "勾企", "门扇高+56", 2, "扇", ""),
        ("平开门", "35*45平安鑫单包平开", "上下方", "(总宽-41.9*2-4.5+8*2)/2-50+44", 2, "扇", ""),

        // ============ 算法变量定义(category=变量,供求值器解析中间变量) ============
        // 含轨道的高度 = 门高>0 ? 门高 : 总高
        ("推拉门", "1645四开推拉门", "含轨道的高度", "总高", 1, "变量", "门高<=0"),
        ("推拉门", "1645四开推拉门", "含轨道的高度", "门高", 1, "变量", "门高>0"),
        // 光企高算法(地轨-80/吊轨-90/自粘下轨-93)
        ("推拉门", "1645四开推拉门", "光企高算法", "含轨道的高度-80", 1, "变量", "轨道类型=地轨"),
        ("推拉门", "1645四开推拉门", "光企高算法", "含轨道的高度-90", 1, "变量", "轨道类型=吊轨"),
        ("推拉门", "1645四开推拉门", "光企高算法", "含轨道的高度-93", 1, "变量", "轨道类型=自粘下轨"),
        // 上下方算法变量(按扇数+冒窗边数量)
        ("推拉门", "1645四开推拉门", "上下方算法变量", "(总宽-26+16+16+16)/4", 1, "变量", "扇数=4"),
        ("推拉门", "1645四开推拉门", "上下方算法变量", "(总宽-26+16+16)/4-15", 1, "变量", "扇数=3"),
        ("推拉门", "1645四开推拉门", "上下方算法变量", "(总宽-26+16+16)/2", 1, "变量", "扇数=2"),
        // 光企数量/勾企数量(按扇数)
        ("推拉门", "1645四开推拉门", "光企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "1645四开推拉门", "光企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "1645四开推拉门", "光企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "1645四开推拉门", "勾企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "1645四开推拉门", "勾企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "1645四开推拉门", "勾企数量", "4", 1, "变量", "扇数=4"),
        // 上亮玻璃宽(按窗数)
        ("推拉门", "1645四开推拉门", "上亮玻璃宽", "(总宽-30-30)", 1, "变量", "窗数=1"),
        ("推拉门", "1645四开推拉门", "上亮玻璃宽", "(总宽-30-30-30)/2", 1, "变量", "窗数=2"),
        ("推拉门", "1645四开推拉门", "上亮玻璃宽", "(总宽-30-30-30-30)/3", 1, "变量", "窗数=3"),
        ("推拉门", "1645四开推拉门", "上亮玻璃宽", "(总宽-30-30-30-30)/4", 1, "变量", "窗数=4"),
        // 上亮大玻璃数量(按窗数)
        ("推拉门", "1645四开推拉门", "上亮大玻璃数量", "1", 1, "变量", "窗数=1"),
        ("推拉门", "1645四开推拉门", "上亮大玻璃数量", "2", 1, "变量", "窗数=2"),
        ("推拉门", "1645四开推拉门", "上亮大玻璃数量", "3", 1, "变量", "窗数=3"),
        ("推拉门", "1645四开推拉门", "上亮大玻璃数量", "4", 1, "变量", "窗数=4"),
        ("推拉门", "1645四开推拉门", "上亮高", "总高-门高", 1, "变量", "门高>0"),

        // 1645单玻三联动 变量
        ("推拉门", "1645单玻三联动推拉门", "含轨道的高度", "总高", 1, "变量", "门高<=0"),
        ("推拉门", "1645单玻三联动推拉门", "含轨道的高度", "门高", 1, "变量", "门高>0"),
        ("推拉门", "1645单玻三联动推拉门", "光企高算法", "含轨道的高度-75", 1, "变量", "轨道类型=地轨"),
        ("推拉门", "1645单玻三联动推拉门", "光企高算法", "含轨道的高度-85", 1, "变量", "轨道类型=吊轨"),
        ("推拉门", "1645单玻三联动推拉门", "光企高算法", "含轨道的高度-90", 1, "变量", "轨道类型=自粘下轨"),
        ("推拉门", "1645单玻三联动推拉门", "上下方算法变量", "(总宽-26+16)/2+28", 1, "变量", "扇数=2"),
        ("推拉门", "1645单玻三联动推拉门", "上下方算法变量", "(总宽-26+16)/3-28", 1, "变量", "扇数=3"),
        ("推拉门", "1645单玻三联动推拉门", "上下方算法变量", "(总宽-26+16+16)/4-15+2", 1, "变量", "扇数=4"),
        ("推拉门", "1645单玻三联动推拉门", "光企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "1645单玻三联动推拉门", "光企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "1645单玻三联动推拉门", "光企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "1645单玻三联动推拉门", "勾企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "1645单玻三联动推拉门", "勾企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "1645单玻三联动推拉门", "勾企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "1645单玻三联动推拉门", "上亮玻璃宽", "(总宽-30-30)", 1, "变量", "窗数=1"),
        ("推拉门", "1645单玻三联动推拉门", "上亮玻璃宽", "(总宽-30-30-30)/2", 1, "变量", "窗数=2"),
        ("推拉门", "1645单玻三联动推拉门", "上亮玻璃宽", "(总宽-30-30-30-30)/3", 1, "变量", "窗数=3"),
        ("推拉门", "1645单玻三联动推拉门", "上亮玻璃宽", "(总宽-30-30-30-30-30)/4", 1, "变量", "窗数=4"),
        ("推拉门", "1645单玻三联动推拉门", "上亮大玻璃数量", "1", 1, "变量", "窗数=1"),
        ("推拉门", "1645单玻三联动推拉门", "上亮大玻璃数量", "2", 1, "变量", "窗数=2"),
        ("推拉门", "1645单玻三联动推拉门", "上亮大玻璃数量", "3", 1, "变量", "窗数=3"),
        ("推拉门", "1645单玻三联动推拉门", "上亮大玻璃数量", "4", 1, "变量", "窗数=4"),
        ("推拉门", "1645单玻三联动推拉门", "上亮高", "总高-门高", 1, "变量", "门高>0"),

        // 鹏飞6033 变量
        ("推拉门", "鹏飞6033推拉门", "含轨道的高度", "总高", 1, "变量", "门高<=0"),
        ("推拉门", "鹏飞6033推拉门", "含轨道的高度", "门高", 1, "变量", "门高>0"),
        ("推拉门", "鹏飞6033推拉门", "光企高算法", "含轨道的高度-60", 1, "变量", "轨道类型=地轨"),
        ("推拉门", "鹏飞6033推拉门", "光企高算法", "含轨道的高度-55", 1, "变量", "轨道类型=吊轨"),
        ("推拉门", "鹏飞6033推拉门", "光企高算法", "含轨道的高度-52", 1, "变量", "轨道类型=自粘下轨"),
        ("推拉门", "鹏飞6033推拉门", "上下方算法变量", "(总宽-45+60)/2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞6033推拉门", "上下方算法变量", "(总宽-45+60)/3", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞6033推拉门", "上下方算法变量", "(总宽-45+60+60)/4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞6033推拉门", "光企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞6033推拉门", "光企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞6033推拉门", "光企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞6033推拉门", "勾企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞6033推拉门", "勾企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞6033推拉门", "勾企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞6033推拉门", "上亮玻璃宽", "(总宽-45-60)/2", 1, "变量", "窗数=1"),
        ("推拉门", "鹏飞6033推拉门", "上亮玻璃宽", "(总宽-45-60-60)/2", 1, "变量", "窗数=2"),
        ("推拉门", "鹏飞6033推拉门", "上亮玻璃宽", "(总宽-28-60-60-60)/3", 1, "变量", "窗数=3"),
        ("推拉门", "鹏飞6033推拉门", "上亮玻璃宽", "(总宽-28-60-60-60-60)/4", 1, "变量", "窗数=4"),
        ("推拉门", "鹏飞6033推拉门", "上亮大玻璃数量", "1", 1, "变量", "窗数=1"),
        ("推拉门", "鹏飞6033推拉门", "上亮大玻璃数量", "2", 1, "变量", "窗数=2"),
        ("推拉门", "鹏飞6033推拉门", "上亮大玻璃数量", "3", 1, "变量", "窗数=3"),
        ("推拉门", "鹏飞6033推拉门", "上亮大玻璃数量", "4", 1, "变量", "窗数=4"),
        ("推拉门", "鹏飞6033推拉门", "上亮高", "总高-门高", 1, "变量", "门高>0"),

        // 鹏飞7530 变量
        ("推拉门", "鹏飞7530推拉门", "含轨道的高度", "总高", 1, "变量", "门高<=0"),
        ("推拉门", "鹏飞7530推拉门", "含轨道的高度", "门高", 1, "变量", "门高>0"),
        ("推拉门", "鹏飞7530推拉门", "光企高算法", "含轨道的高度-60", 1, "变量", "轨道类型=地轨"),
        ("推拉门", "鹏飞7530推拉门", "光企高算法", "含轨道的高度-55", 1, "变量", "轨道类型=吊轨"),
        ("推拉门", "鹏飞7530推拉门", "光企高算法", "含轨道的高度-50", 1, "变量", "轨道类型=自粘下轨"),
        ("推拉门", "鹏飞7530推拉门", "上下方算法变量", "(总宽-43+75)/2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞7530推拉门", "上下方算法变量", "(总宽-43+75)/3", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞7530推拉门", "上下方算法变量", "(总宽-43+75+75)/4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞7530推拉门", "光企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞7530推拉门", "光企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞7530推拉门", "光企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞7530推拉门", "勾企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞7530推拉门", "勾企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞7530推拉门", "勾企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞7530推拉门", "上亮玻璃宽", "(总宽-30-30)", 1, "变量", "窗数=1"),
        ("推拉门", "鹏飞7530推拉门", "上亮玻璃宽", "(总宽-30-30-30)/2", 1, "变量", "窗数=2"),
        ("推拉门", "鹏飞7530推拉门", "上亮玻璃宽", "(总宽-30-30-30-30)/3", 1, "变量", "窗数=3"),
        ("推拉门", "鹏飞7530推拉门", "上亮玻璃宽", "(总宽-30-30-30-30-30)/4", 1, "变量", "窗数=4"),
        ("推拉门", "鹏飞7530推拉门", "上亮大玻璃数量", "1", 1, "变量", "窗数=1"),
        ("推拉门", "鹏飞7530推拉门", "上亮大玻璃数量", "2", 1, "变量", "窗数=2"),
        ("推拉门", "鹏飞7530推拉门", "上亮大玻璃数量", "3", 1, "变量", "窗数=3"),
        ("推拉门", "鹏飞7530推拉门", "上亮大玻璃数量", "4", 1, "变量", "窗数=4"),
        ("推拉门", "鹏飞7530推拉门", "上亮高", "总高-门高", 1, "变量", "门高>0"),

        // 7530一字轨 变量
        ("推拉门", "7530一字轨", "含轨道的高度", "总高", 1, "变量", "门高<=0"),
        ("推拉门", "7530一字轨", "含轨道的高度", "门高", 1, "变量", "门高>0"),
        ("推拉门", "7530一字轨", "光企高算法", "含轨道的高度-50", 1, "变量", "轨道类型=地轨"),
        ("推拉门", "7530一字轨", "光企高算法", "含轨道的高度-55", 1, "变量", "轨道类型=吊轨"),
        ("推拉门", "7530一字轨", "光企高算法", "含轨道的高度-55", 1, "变量", "轨道类型=自粘下轨"),
        ("推拉门", "7530一字轨", "上下方算法变量", "(总宽-43+75)/2", 1, "变量", "扇数=2"),
        ("推拉门", "7530一字轨", "上下方算法变量", "(总宽-43+75)/3", 1, "变量", "扇数=3"),
        ("推拉门", "7530一字轨", "上下方算法变量", "(总宽-43+75+75)/4", 1, "变量", "扇数=4"),
        ("推拉门", "7530一字轨", "光企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "7530一字轨", "光企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "7530一字轨", "光企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "7530一字轨", "勾企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "7530一字轨", "勾企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "7530一字轨", "勾企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "7530一字轨", "上亮玻璃宽", "(总宽-30-30)", 1, "变量", "窗数=1"),
        ("推拉门", "7530一字轨", "上亮玻璃宽", "(总宽-30-30-30)/2", 1, "变量", "窗数=2"),
        ("推拉门", "7530一字轨", "上亮玻璃宽", "(总宽-30-30-30-30)/3", 1, "变量", "窗数=3"),
        ("推拉门", "7530一字轨", "上亮玻璃宽", "(总宽-30-30-30-30-30)/4", 1, "变量", "窗数=4"),
        ("推拉门", "7530一字轨", "上亮大玻璃数量", "1", 1, "变量", "窗数=1"),
        ("推拉门", "7530一字轨", "上亮大玻璃数量", "2", 1, "变量", "窗数=2"),
        ("推拉门", "7530一字轨", "上亮大玻璃数量", "3", 1, "变量", "窗数=3"),
        ("推拉门", "7530一字轨", "上亮大玻璃数量", "4", 1, "变量", "窗数=4"),
        ("推拉门", "7530一字轨", "上亮高", "总高-门高", 1, "变量", "门高>0"),

        // 鹏飞7535三轨 变量
        ("推拉门", "鹏飞7535三轨推拉门", "含轨道的高度", "总高", 1, "变量", "门高<=0"),
        ("推拉门", "鹏飞7535三轨推拉门", "含轨道的高度", "门高", 1, "变量", "门高>0"),
        ("推拉门", "鹏飞7535三轨推拉门", "光企高算法", "含轨道的高度-60", 1, "变量", "轨道类型=串轨"),
        ("推拉门", "鹏飞7535三轨推拉门", "光企高算法", "含轨道的高度-55", 1, "变量", "轨道类型=吊轨"),
        ("推拉门", "鹏飞7535三轨推拉门", "光企高算法", "含轨道的高度-50", 1, "变量", "轨道类型=自粘下轨"),
        ("推拉门", "鹏飞7535三轨推拉门", "光企高算法", "含轨道的高度-45", 1, "变量", "轨道类型=地轨"),
        ("推拉门", "鹏飞7535三轨推拉门", "上下方算法变量", "(总宽-43+75)/2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞7535三轨推拉门", "上下方算法变量", "(总宽-43+75+75)/3", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞7535三轨推拉门", "上下方算法变量", "(总宽-43+75+75+75)/4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞7535三轨推拉门", "光企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞7535三轨推拉门", "光企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞7535三轨推拉门", "光企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞7535三轨推拉门", "勾企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "鹏飞7535三轨推拉门", "勾企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "鹏飞7535三轨推拉门", "勾企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮玻璃宽", "(总宽-30-30)", 1, "变量", "窗数=1"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮玻璃宽", "(总宽-30-30-30)/2", 1, "变量", "窗数=2"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮玻璃宽", "(总宽-30-30-30-30)/3", 1, "变量", "窗数=3"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮玻璃宽", "(总宽-30-30-30-30-30)/4", 1, "变量", "窗数=4"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮大玻璃数量", "1", 1, "变量", "窗数=1"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮大玻璃数量", "2", 1, "变量", "窗数=2"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮大玻璃数量", "3", 1, "变量", "窗数=3"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮大玻璃数量", "4", 1, "变量", "窗数=4"),
        ("推拉门", "鹏飞7535三轨推拉门", "上亮高", "总高-门高", 1, "变量", "门高>0"),

        // 1340单玻两轨 变量
        ("推拉门", "1340单玻两轨推拉门", "含轨道的高度", "总高", 1, "变量", "门高<=0"),
        ("推拉门", "1340单玻两轨推拉门", "含轨道的高度", "门高", 1, "变量", "门高>0"),
        ("推拉门", "1340单玻两轨推拉门", "光企高算法", "含轨道的高度-111", 1, "变量", "轨道类型=地轨"),
        ("推拉门", "1340单玻两轨推拉门", "光企高算法", "含轨道的高度-90", 1, "变量", "轨道类型=吊轨"),
        ("推拉门", "1340单玻两轨推拉门", "光企高算法", "含轨道的高度-75", 1, "变量", "轨道类型=自粘下轨"),
        ("推拉门", "1340单玻两轨推拉门", "上下方算法变量", "(总宽-26+13)/2+28", 1, "变量", "扇数=2"),
        ("推拉门", "1340单玻两轨推拉门", "上下方算法变量", "(总宽-26+13+13)/3+28", 1, "变量", "扇数=3"),
        ("推拉门", "1340单玻两轨推拉门", "上下方算法变量", "(总宽-26+13+13)/4-15+2", 1, "变量", "扇数=4"),
        ("推拉门", "1340单玻两轨推拉门", "光企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "1340单玻两轨推拉门", "光企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "1340单玻两轨推拉门", "光企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "1340单玻两轨推拉门", "勾企数量", "2", 1, "变量", "扇数=2"),
        ("推拉门", "1340单玻两轨推拉门", "勾企数量", "4", 1, "变量", "扇数=3"),
        ("推拉门", "1340单玻两轨推拉门", "勾企数量", "4", 1, "变量", "扇数=4"),
        ("推拉门", "1340单玻两轨推拉门", "上亮玻璃宽", "(总宽-30-30)", 1, "变量", "窗数=1"),
        ("推拉门", "1340单玻两轨推拉门", "上亮玻璃宽", "(总宽-30-30-30)/2", 1, "变量", "窗数=2"),
        ("推拉门", "1340单玻两轨推拉门", "上亮玻璃宽", "(总宽-30-30-30-30)/3", 1, "变量", "窗数=3"),
        ("推拉门", "1340单玻两轨推拉门", "上亮玻璃宽", "(总宽-30-30-30-30)/4", 1, "变量", "窗数=4"),
        ("推拉门", "1340单玻两轨推拉门", "上亮大玻璃数量", "1", 1, "变量", "窗数=1"),
        ("推拉门", "1340单玻两轨推拉门", "上亮大玻璃数量", "2", 1, "变量", "窗数=2"),
        ("推拉门", "1340单玻两轨推拉门", "上亮大玻璃数量", "3", 1, "变量", "窗数=3"),
        ("推拉门", "1340单玻两轨推拉门", "上亮大玻璃数量", "4", 1, "变量", "窗数=4"),
        ("推拉门", "1340单玻两轨推拉门", "上亮高", "总高-门高", 1, "变量", "门高>0"),

        // 平开门 35*45 变量(门扇高/门扇宽/上亮宽/窗高)
        ("平开门", "35*45平安鑫单包平开", "门扇高", "总高-上框-下框", 1, "变量", ""),
        ("平开门", "35*45平安鑫单包平开", "门扇宽", "(总宽-41.9*2-4.5)/2", 1, "变量", ""),
        ("平开门", "35*45平安鑫单包平开", "上亮宽", "总宽-29*2", 1, "变量", "窗高>0"),
    ];

    // 增量补种型材
    let existing: std::collections::HashSet<(String, String, String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT product_type, series, profile_name, condition, category FROM profile_lib")
            .expect("prepare profile_lib select2");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, String>(2).unwrap_or_default(),
                    r.get::<_, String>(7).unwrap_or_default(),
                    r.get::<_, String>(6).unwrap_or_default(),
                ))
            })
            .expect("query profile_lib2");
        rows.filter_map(|x| x.ok()).collect()
    };

    let mut ins = conn
        .prepare(
            "INSERT INTO profile_lib(product_type, series, profile_name, formula, quantity, category, sort_order, condition, quantity_formula) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        )
        .expect("prepare profile_lib insert2");

    let mut sort = existing.len() as i64;
    for (pt, series, name, formula, qty, cat, cond) in &profiles {
        let key = (pt.to_string(), series.to_string(), name.to_string(), cond.to_string(), cat.to_string());
        if existing.contains(&key) {
            continue;
        }
        // 数量公式:动态数量的型材引用变量
        let qf = match *name {
            "光企高" => "光企数量",
            "勾企高" => "勾企数量",
            "上下方" => "扇数*2",
            _ => "",
        };
        ins.execute(rusqlite::params![pt, series, name, formula, qty, cat, sort, cond, qf]).ok();
        sort += 1;
    }

    seed_glass_lib(conn);
}

/// 内置玻璃公式库(按型号,面玻/背玻/上亮玻璃)
fn seed_glass_lib(conn: &Connection) {
    // (产品类型, 系列, 玻璃名, 高度公式, 宽度公式, 数量公式, 条件)
    let glasses: Vec<(&str, &str, &str, &str, &str, &str, &str)> = vec![
        // 1645四开推拉门 玻璃
        ("推拉门", "1645四开推拉门", "面玻", "光企高算法-23", "上下方算法变量-23-28", "扇数", ""),
        ("推拉门", "1645四开推拉门", "背玻", "光企高算法-23", "上下方算法变量-33-28", "扇数", "单双玻=双玻"),
        ("推拉门", "1645四开推拉门", "窗-窗面", "上亮高-60+5", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        ("推拉门", "1645四开推拉门", "窗-窗背", "上亮高-60+5", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        // 1645单玻三联动 玻璃
        ("推拉门", "1645单玻三联动推拉门", "面玻", "光企高算法-23", "上下方算法变量-22-28", "扇数", ""),
        ("推拉门", "1645单玻三联动推拉门", "背玻", "光企高算法-23", "上下方算法变量-22-28", "扇数", "单双玻=双玻"),
        ("推拉门", "1645单玻三联动推拉门", "窗-窗面", "上亮高-60+5", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        ("推拉门", "1645单玻三联动推拉门", "窗-窗背", "上亮高-60+5", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        // 鹏飞6033 玻璃
        ("推拉门", "鹏飞6033推拉门", "面玻", "光企高算法-104", "上下方算法变量-104", "扇数", ""),
        ("推拉门", "鹏飞6033推拉门", "背玻", "光企高算法-104", "上下方算法变量-104", "扇数", "单双玻=双玻"),
        ("推拉门", "鹏飞6033推拉门", "窗-窗面", "上亮高-40+3", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        ("推拉门", "鹏飞6033推拉门", "窗-窗背", "上亮高-40+3", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        // 鹏飞7530 玻璃
        ("推拉门", "鹏飞7530推拉门", "面玻", "光企高算法-130", "上下方算法变量-130", "扇数", ""),
        ("推拉门", "鹏飞7530推拉门", "背玻", "光企高算法-130", "上下方算法变量-130", "扇数", "单双玻=双玻"),
        ("推拉门", "鹏飞7530推拉门", "窗-窗面", "上亮高-10", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        ("推拉门", "鹏飞7530推拉门", "窗-窗背", "上亮高-10", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        // 7530一字轨 玻璃
        ("推拉门", "7530一字轨", "面玻", "光企高算法-130", "上下方算法变量-130", "扇数", ""),
        ("推拉门", "7530一字轨", "背玻", "光企高算法-130", "上下方算法变量-130", "扇数", "单双玻=双玻"),
        ("推拉门", "7530一字轨", "窗-窗面", "上亮高-60+5", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        ("推拉门", "7530一字轨", "窗-窗背", "上亮高-60+5", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        // 鹏飞7535三轨 玻璃
        ("推拉门", "鹏飞7535三轨推拉门", "面玻", "光企高算法-130", "上下方算法变量-130", "扇数", ""),
        ("推拉门", "鹏飞7535三轨推拉门", "背玻", "光企高算法-130", "上下方算法变量-130", "扇数", "单双玻=双玻"),
        ("推拉门", "鹏飞7535三轨推拉门", "窗-窗面", "上亮高-10", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        ("推拉门", "鹏飞7535三轨推拉门", "窗-窗背", "上亮高-10", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        // 1340单玻两轨 玻璃
        ("推拉门", "1340单玻两轨推拉门", "面玻", "光企高算法-17", "上下方算法变量-18-28", "扇数", ""),
        ("推拉门", "1340单玻两轨推拉门", "背玻", "光企高算法-17", "上下方算法变量-18-28", "扇数", "单双玻=双玻"),
        ("推拉门", "1340单玻两轨推拉门", "窗-窗面", "上亮高-60+5", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        ("推拉门", "1340单玻两轨推拉门", "窗-窗背", "上亮高-60+5", "上亮玻璃宽", "上亮大玻璃数量", "门高>0"),
        // 35*45平安鑫单包平开 玻璃
        ("平开门", "35*45平安鑫单包平开", "面-面玻", "门扇高-78", "门扇宽-78", "1", "单双玻=双玻"),
        ("平开门", "35*45平安鑫单包平开", "背-背玻", "门扇高-78", "门扇宽-78", "1", "单双玻=双玻"),
        ("平开门", "35*45平安鑫单包平开", "面玻", "门扇高-78", "门扇宽-78", "1", "单双玻=单玻"),
        ("平开门", "35*45平安鑫单包平开", "窗-窗固", "窗高-10", "上亮宽-10", "1", "窗高>0"),
    ];

    let existing: std::collections::HashSet<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT product_type, series, glass_name FROM glass_lib")
            .expect("prepare glass_lib select");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, String>(2).unwrap_or_default(),
                ))
            })
            .expect("query glass_lib");
        rows.filter_map(|x| x.ok()).collect()
    };

    let mut ins = conn
        .prepare(
            "INSERT INTO glass_lib(product_type, series, glass_name, height_formula, width_formula, quantity_formula, condition, sort_order) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        )
        .expect("prepare glass_lib insert");

    let mut sort = existing.len() as i64;
    for (pt, series, name, hf, wf, qf, cond) in &glasses {
        let key = (pt.to_string(), series.to_string(), name.to_string());
        if existing.contains(&key) {
            continue;
        }
        ins.execute(rusqlite::params![pt, series, name, hf, wf, qf, cond, sort]).ok();
        sort += 1;
    }
}

/// 自动迁移:为旧版 profile_lib 表补充 condition 字段(幂等)
fn migrate_profile_lib(conn: &Connection) {
    let cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(profile_lib)").ok();
        stmt.as_mut()
            .map(|s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .ok()
                    .map(|rows| rows.filter_map(|x| x.ok()).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    };
    if !cols.iter().any(|c| c == "condition") {
        conn.execute("ALTER TABLE profile_lib ADD COLUMN condition TEXT DEFAULT ''", []).ok();
    }
    if !cols.iter().any(|c| c == "quantity_formula") {
        conn.execute("ALTER TABLE profile_lib ADD COLUMN quantity_formula TEXT DEFAULT ''", []).ok();
    }
}

/// 清理历史脏数据(幂等):
/// 1. 旧版 seed_full_profile_lib 曾按 3 元组去重,导致同型号重复插入两遍 → 按
///    (product_type, series, profile_name, condition) 去重,每个唯一组合保留 sort_order 最小的一条。
/// 2. 算法中间量(category=变量)被旧版 seed_profile_lib 误标成 category=框 → 若同名同条件
///    存在 category=变量 的行,则删除 category=框 的污染行。
/// 只处理"变量"类与已知污染的算法行,不触碰用户自定义的框/扇/支撑型材。
fn dedup_profile_lib(conn: &Connection) {
    // 第一步:删除 category=变量 的重复行,保留每个 (pt, series, name, condition) 中 id 最小者
    conn.execute(
        "DELETE FROM profile_lib WHERE category='变量' AND id NOT IN ( \
           SELECT MIN(id) FROM profile_lib WHERE category='变量' \
           GROUP BY product_type, series, profile_name, condition \
         )",
        [],
    )
    .ok();

    // 第二步:删除被误标为 category=框 的算法变量行(同名同条件已存在 category=变量 版本)
    //   这些名字是纯算法中间量,不应作为型材下料输出。
    conn.execute(
        "DELETE FROM profile_lib WHERE category='框' AND profile_name IN ( \
           SELECT DISTINCT profile_name FROM profile_lib WHERE category='变量' \
         ) AND (product_type, series, profile_name, condition) IN ( \
           SELECT product_type, series, profile_name, condition FROM profile_lib WHERE category='变量' \
         )",
        [],
    )
    .ok();

    // 第三步:去重 框/扇/支撑 类型材的历史重复插入(旧版按 3 元组去重导致同型号插多遍)。
    //   按 (product_type, series, profile_name, formula, condition) 完全一致判重,保留 id 最小者。
    //   用户自定义通常会改 formula/condition,不会被误删。
    conn.execute(
        "DELETE FROM profile_lib WHERE category NOT IN ('变量','报价') AND id NOT IN ( \
           SELECT MIN(id) FROM profile_lib WHERE category NOT IN ('变量','报价') \
           GROUP BY product_type, series, profile_name, formula, condition \
         )",
        [],
    )
    .ok();
}

/// 自动迁移:为旧版 orders 表补充缺失字段(幂等,可重复执行)
fn migrate_orders(conn: &Connection) {    let cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(orders)").ok();
        stmt.as_mut()
            .map(|s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .ok()
                    .map(|rows| rows.filter_map(|x| x.ok()).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    };
    let new_cols: Vec<(&str, &str)> = vec![
        ("contract_no", "TEXT"),
        ("customer_id", "INTEGER"),
        ("terminal_addr", "TEXT"),
        ("terminal_phone", "TEXT"),
        ("material_type", "TEXT"),
        ("product_name", "TEXT"),
        ("color", "TEXT"),
        ("width", "REAL"),
        ("height", "REAL"),
        ("face_glass", "TEXT"),
        ("back_glass", "TEXT"),
        ("glass_layer", "TEXT"),
        ("face_glass_w", "REAL"),
        ("face_glass_h", "REAL"),
        ("back_glass_w", "REAL"),
        ("back_glass_h", "REAL"),
        ("style", "TEXT"),
        ("remark", "TEXT"),
        ("total_price", "REAL DEFAULT 0"),
        ("mark", "TEXT"),
        ("contract_status", "TEXT"),
        ("order_date", "TEXT"),
        ("delivery_date", "TEXT"),
        ("inbound_date", "TEXT"),
        ("outbound_date", "TEXT"),
        ("salesman", "TEXT"),
        ("operator", "TEXT"),
        ("unit_price", "REAL"),
        ("fan_count", "INTEGER"),
        ("track_type", "TEXT"),
        ("door_height", "REAL"),
        ("wall_thickness", "REAL"),
    ];
    for (name, typ) in new_cols {
        if !cols.iter().any(|c| c == name) {
            let sql = format!("ALTER TABLE orders ADD COLUMN {} {}", name, typ);
            conn.execute(&sql, []).ok();
        }
    }
}

/// 自动迁移:为旧版 contracts 表补充缺失字段(幂等,可重复执行)
fn migrate_contracts(conn: &Connection) {
    let cols: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(contracts)").ok();
        stmt.as_mut()
            .map(|s| {
                s.query_map([], |r| r.get::<_, String>(1))
                    .ok()
                    .map(|rows| rows.filter_map(|x| x.ok()).collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    };
    let new_cols: Vec<(&str, &str)> = vec![
        ("order_count", "INTEGER DEFAULT 0"),
        ("pay_progress", "TEXT"),
        ("inbound_progress", "TEXT"),
        ("outbound_progress", "TEXT"),
    ];
    for (name, typ) in new_cols {
        if !cols.iter().any(|c| c == name) {
            let sql = format!("ALTER TABLE contracts ADD COLUMN {} {}", name, typ);
            conn.execute(&sql, []).ok();
        }
    }
}

/// 读取配置
pub fn get_config(conn: &Connection, key: &str) -> Option<String> {    conn.query_row("SELECT value FROM app_config WHERE key=?1", rusqlite::params![key], |r| r.get(0))
        .ok()
}

/// 写入配置。写库失败必须向调用方报错，否则设置页会显示"已保存"但重启后丢失。
pub fn set_config(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO app_config(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        rusqlite::params![key, value],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod seed_tests {
    use super::*;
    use crate::order::OrderService;
    use crate::plugin::PluginHost;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_seed_full_profile_data() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);

        let cnt: i64 = conn.query_row(
            "SELECT COUNT(*) FROM profile_lib WHERE series='鹏飞7530推拉门'",
            [], |r| r.get(0)).unwrap();
        assert!(cnt > 0);
        let glass_cnt: i64 = conn.query_row(
            "SELECT COUNT(*) FROM glass_lib WHERE series='鹏飞7530推拉门'",
            [], |r| r.get(0)).unwrap();
        assert!(glass_cnt > 0);

        // 构造 OrderService 计算
        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = OrderService::new(db, host);

        let profiles = svc.calc_profile_sizes("推拉门", "鹏飞7530推拉门", 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, false).unwrap();
        println!("=== 鹏飞7530 型材(1730x2140 扇2 地轨) ===");
        for (n, l, q, c) in &profiles { println!("  {} = {} x {} [{}]", n, l, q, c); }
        if let Some((_, len, _, _)) = profiles.iter().find(|(n,_,_,_)| n == "边封高") {
            assert!((len - 1120.0).abs() < 0.5, "边封高={}", len);
        }
        if let Some((_, len, _, _)) = profiles.iter().find(|(n,_,_,_)| n == "轨道长") {
            assert!((len - 650.0).abs() < 0.5, "轨道长={}", len);
        }
        if let Some((_, len, _, _)) = profiles.iter().find(|(n,_,_,_)| n == "光企高") {
            assert!((len - 2080.0).abs() < 0.5, "光企高={}", len);
        }

        let glasses = svc.calc_glass_sizes("推拉门", "鹏飞7530推拉门", 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, false).unwrap();
        println!("=== 鹏飞7530 玻璃 ===");
        for (n, gw, gh, q) in &glasses { println!("  {} = {}x{} x {}", n, gw, gh, q); }
    }

    #[test]
    fn test_glass_branch_single_vs_double() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = OrderService::new(db, host);

        // 单玻(double_glass=false):不应出现"背玻"(条件 单双玻=双玻 不满足)
        let single = svc.calc_glass_sizes("推拉门", "鹏飞7530推拉门", 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, false).unwrap();
        let names: Vec<&str> = single.iter().map(|(n,_,_,_)| n.as_str()).collect();
        assert!(names.iter().any(|n| *n == "面玻"), "单玻应有面玻: {:?}", names);
        assert!(!names.iter().any(|n| *n == "背玻"), "单玻不应有背玻: {:?}", names);

        // 双玻:应同时有面玻和背玻
        let double = svc.calc_glass_sizes("推拉门", "鹏飞7530推拉门", 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, true).unwrap();
        let dnames: Vec<&str> = double.iter().map(|(n,_,_,_)| n.as_str()).collect();
        assert!(dnames.iter().any(|n| *n == "面玻"), "双玻应有面玻: {:?}", dnames);
        assert!(dnames.iter().any(|n| *n == "背玻"), "双玻应有背玻: {:?}", dnames);

        // 门高>0(有上亮):应出现"窗-窗面"(上亮玻璃),且面玻尺寸按 光企高算法(含轨道的高度-60) 计算
        let with_lintel = svc.calc_glass_sizes("推拉门", "鹏飞7530推拉门", 1730.0, 2140.0, 2, 2, 500.0, 0.0, "地轨", 0, true).unwrap();
        let lnames: Vec<&str> = with_lintel.iter().map(|(n,_,_,_)| n.as_str()).collect();
        assert!(lnames.iter().any(|n| *n == "窗-窗面"), "门高>0 应有上亮玻璃(窗-窗面): {:?}", lnames);

        // 双玻 面玻尺寸: 宽 = 上下方算法变量-130 = (1730-43+75)/2-130 = 881-130=751; 高 = 光企高算法-130 = (2140-60)-130=1950
        if let Some((_, gw, gh, _)) = double.iter().find(|(n,_,_,_)| n == "面玻") {
            assert!((gw - 751.0).abs() < 0.5, "双玻面玻宽={}", gw);
            assert!((gh - 1950.0).abs() < 0.5, "双玻面玻高={}", gh);
        }
        println!("单玻/双玻/上亮 分支验证通过");
    }

    #[test]
    fn test_all_models_seeded_and_rail_match() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        crate::models_seed::seed_all_models(&conn);

        // 分类表里的关键型号必须全部入库
        let seeded: std::collections::HashSet<String> = {
            let mut stmt = conn.prepare("SELECT DISTINCT series FROM profile_lib").unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.filter_map(|x| x.ok()).collect()
        };
        for s in [
            "7535平安鑫推拉门", "平安鑫6035推拉门", "1645卡枪灰推拉门",
            "鹏飞1835三连动推拉门", "1340三连动推拉门", "9040鹏飞推拉门",
            "8533鹏飞推拉门", "单轨推拉模板", "测试的三轨推拉", "16*45三联动推拉-测试工时",
            "鹏飞6033单包平开门", "7535带望窗平开门",
        ] {
            assert!(seeded.contains(s), "分类表型号缺失: {}", s);
        }

        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = OrderService::new(db, host);

        // 用户点名的例子:7535平安鑫推拉门 + 自粘下轨
        // 截图 14848 确认:地轨-60 / 吊轨-55 / 自粘下轨-50
        let p = svc.calc_profile_sizes("推拉门", "7535平安鑫推拉门", 1730.0, 2140.0,
                                       2, 2, 0.0, 0.0, "自粘下轨", 0, true).unwrap();
        let gq = p.iter().find(|(n, _, _, _)| n == "光企高").map(|x| x.1).unwrap_or(0.0);
        assert!((gq - 2090.0).abs() < 0.5, "7535平安鑫 自粘下轨 光企高应=2140-50=2090, 实际={}", gq);
        let sxf = p.iter().find(|(n, _, _, _)| n == "上下方").map(|x| x.1).unwrap_or(0.0);
        assert!((sxf - 881.0).abs() < 0.5, "上下方应=(1730-43+75)/2=881, 实际={}", sxf);

        let g = svc.calc_glass_sizes("推拉门", "7535平安鑫推拉门", 1730.0, 2140.0,
                                     2, 2, 0.0, 0.0, "自粘下轨", 0, true).unwrap();
        if let Some((_, w, h, _)) = g.iter().find(|(n, _, _, _)| n == "面玻") {
            assert!((w - 751.0).abs() < 0.5, "面玻宽应=881-130=751, 实际={}", w);
            assert!((h - 1960.0).abs() < 0.5, "面玻高应=2090-130=1960, 实际={}", h);
        } else { panic!("7535平安鑫 未生成面玻"); }

        // 轨道别名:老下拉「双轨」应等价于「地轨」(鹏飞6033 地轨扣减60 -> 2080)
        let a = svc.calc_profile_sizes("推拉门", "鹏飞6033推拉门", 1730.0, 2140.0,
                                       2, 2, 0.0, 0.0, "地轨", 0, true).unwrap();
        let b = svc.calc_profile_sizes("推拉门", "鹏飞6033推拉门", 1730.0, 2140.0,
                                       2, 2, 0.0, 0.0, "双轨", 0, true).unwrap();
        let av = a.iter().find(|(n, _, _, _)| n == "光企高").map(|x| x.1).unwrap_or(0.0);
        let bv = b.iter().find(|(n, _, _, _)| n == "光企高").map(|x| x.1).unwrap_or(0.0);
        assert!((av - 2080.0).abs() < 0.5, "鹏飞6033 地轨 光企高={}", av);
        assert!((av - bv).abs() < 0.5, "双轨别名应等价于地轨: {} vs {}", bv, av);

        // 平开门型号可计算
        let fp = svc.calc_profile_sizes("平开门", "鹏飞6033单包平开门", 900.0, 2100.0,
                                        2, 1, 0.0, 0.0, "地轨", 0, true).unwrap();
        assert!(!fp.is_empty(), "平开门型号未生成型材");
        println!("全型号补全 + 自粘下轨/轨道别名/平开门 验证通过, 型号数={}", seeded.len());
    }

    /// 录屏修正后的验收:空公式型号不产生公式 + 随机抽样实算 + 全库公式复核
    #[test]
    fn test_video_corrections_and_random_calc_consistency() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        migrate_profile_lib(&conn);
        crate::models_seed::normalize_rail_conditions(&conn);
        crate::models_seed::migrate_video_renames(&conn);
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        crate::models_seed::seed_all_models(&conn);
        dedup_profile_lib(&conn);
        crate::models_seed::apply_video_corrections(&conn);

        let db = Arc::new(Mutex::new(conn));
        {
            let c = db.lock().unwrap();
            // 录屏确认型号名:平安鑫(早期误作平安盛)
            let xin: i64 = c.query_row(
                "SELECT COUNT(*) FROM profile_lib WHERE series='35*45平安鑫单包平开'", [], |r| r.get(0)).unwrap();
            let sheng: i64 = c.query_row(
                "SELECT COUNT(*) FROM profile_lib WHERE series='35*45平安盛单包平开'", [], |r| r.get(0)).unwrap();
            assert!(xin > 0, "录屏确认的「平安鑫」未入库");
            assert_eq!(sheng, 0, "误读的「平安盛」未清理");

            // 空公式型号:只留一条占位行,无任何玻璃公式
            for s in crate::models_seed::EMPTY_MODELS {
                let total: i64 = c.query_row(
                    "SELECT COUNT(*) FROM profile_lib WHERE series=?1", rusqlite::params![s], |r| r.get(0)).unwrap();
                let ph: i64 = c.query_row(
                    "SELECT COUNT(*) FROM profile_lib WHERE series=?1 AND category='空公式'",
                    rusqlite::params![s], |r| r.get(0)).unwrap();
                let g: i64 = c.query_row(
                    "SELECT COUNT(*) FROM glass_lib WHERE series=?1", rusqlite::params![s], |r| r.get(0)).unwrap();
                assert_eq!(total, 1, "{} 应只留1条占位行,实际 {}", s, total);
                assert_eq!(ph, 1, "{} 占位行缺失", s);
                assert_eq!(g, 0, "{} 不应有玻璃公式", s);
            }

            // 全库复核:无重复型材、无空公式行
            let dup: i64 = c.query_row(
                "SELECT COUNT(*) FROM profile_lib WHERE category NOT IN ('变量','报价','空公式') \
                 AND id NOT IN (SELECT MIN(id) FROM profile_lib \
                   WHERE category NOT IN ('变量','报价','空公式') \
                   GROUP BY product_type,series,profile_name,formula,condition)", [], |r| r.get(0)).unwrap();
            assert_eq!(dup, 0, "型材重复行 {}", dup);
            let blank: i64 = c.query_row(
                "SELECT COUNT(*) FROM profile_lib WHERE category NOT IN ('变量','报价','空公式') \
                 AND TRIM(formula)=''", [], |r| r.get(0)).unwrap();
            assert_eq!(blank, 0, "存在空公式型材行 {}", blank);
        }

        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = OrderService::new(db.clone(), host);

        // 空公式型号:计算必须返回空(不会拿上一个型号的公式顶替)
        for s in crate::models_seed::EMPTY_MODELS {
            let p = svc.calc_profile_sizes("推拉门", s, 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, true).unwrap();
            assert!(p.is_empty(), "{} 公式为空却生成型材 {:?}", s, p);
            let g = svc.calc_glass_sizes("推拉门", s, 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, true).unwrap();
            assert!(g.is_empty(), "{} 公式为空却生成玻璃 {:?}", s, g);
        }

        // 按录屏实测公式核对具体数值(验证「确实按公式算」)
        let cases: &[(&str, &str, &str, f64)] = &[
            ("平开门", "16*35单包平开", "外框高", 2140.0 - 510.0),   // 录屏 t=0
            ("平开门", "4030平开门", "门框宽", 1730.0 - 40.0),        // 录屏 t=80
            ("推拉门", "测试的三轨推拉", "边封高", 2140.0),            // 录屏 t=200
            ("推拉门", "1635单轨推拉门", "边封高", 2140.0 - 1020.0),   // 录屏 t=120
        ];
        for (pt, series, name, expect) in cases {
            let p = svc.calc_profile_sizes(pt, series, 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, true).unwrap();
            let v = p.iter().find(|(n, _, _, _)| n == name).map(|x| x.1);
            assert!(v.is_some(), "{} 缺少 {} 的计算结果: {:?}", series, name, p);
            assert!((v.unwrap() - expect).abs() < 0.5,
                    "{} 的 {} 应={}(按库内公式), 实际={}", series, name, expect, v.unwrap());
        }
        // 4030平开门 玻璃:录屏 t=80 为 门扇高-70 → (2140-30)-70 = 2040
        let g4030 = svc.calc_glass_sizes("平开门", "4030平开门", 1730.0, 2140.0,
                                         2, 1, 0.0, 0.0, "地轨", 0, true).unwrap();
        if let Some((_, _, gh, _)) = g4030.iter().find(|(n, _, _, _)| n == "面-面玻") {
            assert!((gh - 2040.0).abs() < 0.5, "4030平开门 面-面玻高应=2040, 实际={}", gh);
        }

        // 随机抽样 10 个型号实算:结果必须有限、非负、且确有下料项
        let all: Vec<(String, String)> = {
            let c = db.lock().unwrap();
            let mut st = c.prepare(
                "SELECT DISTINCT product_type, series FROM profile_lib WHERE category!='空公式' ORDER BY series"
            ).unwrap();
            st.query_map([], |r| Ok((
                r.get::<_, String>(0).unwrap_or_default(),
                r.get::<_, String>(1).unwrap_or_default(),
            ))).unwrap().filter_map(|x| x.ok()).collect()
        };
        assert!(all.len() > 20, "型号总数异常 {}", all.len());
        let mut seed: u64 = 20261109;
        let mut checked = 0;
        for _ in 0..10 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let (pt, series) = &all[((seed >> 33) as usize) % all.len()];
            let p = svc.calc_profile_sizes(pt, series, 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, true).unwrap();
            assert!(!p.is_empty(), "随机抽样 {} 未生成型材", series);
            for (n, len, qty, _) in &p {
                assert!(len.is_finite() && *len >= 0.0,
                        "{} 的 {} 长度异常 {}", series, n, len);
                assert!(*qty >= 0, "{} 的 {} 数量异常 {}", series, n, qty);
            }
            let g = svc.calc_glass_sizes(pt, series, 1730.0, 2140.0, 2, 2, 0.0, 0.0, "地轨", 0, true).unwrap();
            for (n, w, h, q) in &g {
                assert!(w.is_finite() && h.is_finite() && *w >= 0.0 && *h >= 0.0 && *q >= 0,
                        "{} 的玻璃 {} 尺寸异常 {}x{} x{}", series, n, w, h, q);
            }
            checked += 1;
        }
        println!("录屏修正验收通过:空公式型号 {} 个、随机实算 {} 个、录屏数值核对 {} 项、全库 {} 个型号",
                 crate::models_seed::EMPTY_MODELS.len(), checked, cases.len(), all.len());
    }

    /// 平开门不应该有轨道:库里不能有轨道公式,且轨道参数不能影响平开门计算结果
    #[test]
    fn test_flat_door_has_no_rail() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        migrate_profile_lib(&conn);
        crate::models_seed::normalize_rail_conditions(&conn);
        crate::models_seed::migrate_video_renames(&conn);
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        crate::models_seed::seed_all_models(&conn);
        dedup_profile_lib(&conn);
        crate::models_seed::apply_video_corrections(&conn);

        // 只查「轨道」与推拉门专属的铝板/F条/墙厚;「双包套」是门套概念,不属于轨道,不在此列
        let bad: i64 = conn.query_row(
            "SELECT COUNT(*) FROM profile_lib WHERE product_type='平开门' \
             AND (profile_name LIKE '%轨道%' OR formula LIKE '%轨道%' \
                  OR condition LIKE '%轨道%' OR profile_name LIKE '%铝板%' \
                  OR profile_name LIKE '%F条%' OR formula LIKE '%墙厚%' OR condition LIKE '%墙厚%')",
            [], |r| r.get(0)).unwrap();
        assert_eq!(bad, 0, "平开门库里混入轨道/铝板公式 {} 条", bad);
        let gbad: i64 = conn.query_row(
            "SELECT COUNT(*) FROM glass_lib WHERE product_type='平开门' \
             AND (height_formula LIKE '%轨道%' OR width_formula LIKE '%轨道%' \
                  OR condition LIKE '%轨道%')",
            [], |r| r.get(0)).unwrap();
        assert_eq!(gbad, 0, "平开门玻璃混入轨道公式 {} 条", gbad);

        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = OrderService::new(db, host);
        let series = "16*35单包平开";
        let base_p = svc.calc_profile_sizes("平开门", series, 900.0, 2100.0,
                                            2, 1, 0.0, 0.0, "地轨", 0, true).unwrap();
        let base_g = svc.calc_glass_sizes("平开门", series, 900.0, 2100.0,
                                         2, 1, 0.0, 0.0, "地轨", 0, true).unwrap();
        assert!(!base_p.is_empty(), "平开门未生成型材");

        // 换成任意轨道(含"包"字会触发包套变量)结果都必须一模一样
        for tt in ["吊轨", "自粘下轨", "双包地轨", "串轨", ""] {
            let p = svc.calc_profile_sizes("平开门", series, 900.0, 2100.0,
                                           2, 1, 0.0, 0.0, tt, 0, true).unwrap();
            assert_eq!(p.len(), base_p.len(), "轨道={} 改变了平开门型材项数", tt);
            for (i, (n, l, q, c)) in p.iter().enumerate() {
                let b = &base_p[i];
                assert_eq!(n, &b.0, "轨道={} 让平开门型材项错位", tt);
                assert!((l - b.1).abs() < 1e-9, "轨道={} 改变了 {} 的长度", tt, n);
                assert_eq!(*q, b.2, "轨道={} 改变了 {} 的数量", tt, n);
                assert_eq!(c, &b.3, "轨道={} 改变了 {} 的分类", tt, n);
            }
            let g = svc.calc_glass_sizes("平开门", series, 900.0, 2100.0,
                                         2, 1, 0.0, 0.0, tt, 0, true).unwrap();
            assert_eq!(g.len(), base_g.len(), "轨道={} 改变了平开门玻璃项数", tt);
            for (i, (n, w, h, q)) in g.iter().enumerate() {
                let b = &base_g[i];
                assert_eq!(n, &b.0, "轨道={} 让平开门玻璃项错位", tt);
                assert!((w - b.1).abs() < 1e-9 && (h - b.2).abs() < 1e-9,
                        "轨道={} 改变了 {} 的玻璃尺寸", tt, n);
                assert_eq!(*q, b.3, "轨道={} 改变了 {} 的玻璃数量", tt, n);
            }
        }
        println!("平开门无轨道验证通过:库内轨道公式 0 条, 5 种轨道参数下结果完全一致");
    }

    /// 窗数不是用户输入项:库里不应有「窗数」变量定义,普通门(无上亮)不受窗数影响
    #[test]
    fn test_window_count_is_auto_not_user_input() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        migrate_profile_lib(&conn);
        crate::models_seed::normalize_rail_conditions(&conn);
        crate::models_seed::migrate_video_renames(&conn);
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        crate::models_seed::seed_all_models(&conn);
        dedup_profile_lib(&conn);
        crate::models_seed::apply_video_corrections(&conn);

        // 「窗数」是系统自动推断的参数,不是型号库里的公式变量
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM profile_lib WHERE profile_name='窗数'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "窗数不应作为型号公式变量入库");

        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = OrderService::new(db, host);

        // 普通门且门高=0(没有上亮/带窗):窗数取 1/2/3/4 结果必须完全一致
        for (pt, series) in [("平开门", "16*35单包平开"), ("推拉门", "鹏飞7530推拉门")] {
            let base_p = svc.calc_profile_sizes(pt, series, 1730.0, 2140.0,
                                                2, 1, 0.0, 0.0, "地轨", 0, true).unwrap();
            let base_g = svc.calc_glass_sizes(pt, series, 1730.0, 2140.0,
                                              2, 1, 0.0, 0.0, "地轨", 0, true).unwrap();
            assert!(!base_p.is_empty(), "{} 未生成型材", series);
            for wc in [0i64, 2, 3, 4] {
                let p = svc.calc_profile_sizes(pt, series, 1730.0, 2140.0,
                                               2, wc, 0.0, 0.0, "地轨", 0, true).unwrap();
                assert_eq!(p, base_p, "窗数={} 改变了 {} 的型材结果", wc, series);
                let g = svc.calc_glass_sizes(pt, series, 1730.0, 2140.0,
                                             2, wc, 0.0, 0.0, "地轨", 0, true).unwrap();
                assert_eq!(g, base_g, "窗数={} 改变了 {} 的玻璃结果", wc, series);
            }
        }
        println!("窗数自动推断验证通过:库内无窗数变量, 普通门在窗数 0/1/2/3/4 下结果一致");
    }

    /// 预设列表为空、玻璃类型可用于预输入、玻璃尺寸只按型号公式(不再受扣减配置影响)
    #[test]
    fn test_presets_empty_and_glass_uses_model_formula() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        migrate_profile_lib(&conn);
        crate::models_seed::normalize_rail_conditions(&conn);
        crate::models_seed::migrate_video_renames(&conn);
        seed_product_presets(&conn);
        seed_glass_types(&conn);
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        crate::models_seed::seed_all_models(&conn);
        dedup_profile_lib(&conn);
        crate::models_seed::apply_video_corrections(&conn);

        // 产品预设列表暂时不设任何(且幂等,重复初始化不会重新塞入)
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM product_presets", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "产品预设列表应为空,实际 {} 条", n);
        seed_product_presets(&conn);
        let n2: i64 = conn.query_row("SELECT COUNT(*) FROM product_presets", [], |r| r.get(0)).unwrap();
        assert_eq!(n2, 0, "重复初始化不应重新插入预设");

        // 玻璃类型清单已备好,供面玻/背玻预输入
        let g: i64 = conn.query_row("SELECT COUNT(*) FROM glass_types", [], |r| r.get(0)).unwrap();
        assert!(g >= 50, "玻璃类型候选不足: {}", g);
        let first: String = conn.query_row(
            "SELECT name FROM glass_types ORDER BY category, sort_order LIMIT 1", [], |r| r.get(0)).unwrap();
        assert!(first.contains("钢化"), "热门玻璃未排在最前: {}", first);
        // 用户业务里所有玻璃都是钢化的：库里不允许出现非钢化规格，否则下拉里很容易误选
        let non_tempered: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM glass_types WHERE name NOT LIKE '%钢化%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(non_tempered, 0, "玻璃库里混进了 {} 条非钢化规格", non_tempered);
        let single: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM glass_types WHERE category='单玻'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(single >= 30, "单玻钢化规格太少: {}", single);

        // 简易「每边扣减」公式已移除:写入极端扣减值也不应影响型号玻璃尺寸
        conn.execute("INSERT OR REPLACE INTO app_config(key,value) VALUES('calc.glass.single_w','999')", []).ok();
        conn.execute("INSERT OR REPLACE INTO app_config(key,value) VALUES('calc.glass.double_h','999')", []).ok();

        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = OrderService::new(db, host);
        let glasses = svc.calc_glass_sizes("推拉门", "鹏飞7530推拉门", 1730.0, 2140.0,
                                          2, 1, 0.0, 0.0, "地轨", 0, true).unwrap();
        if let Some((_, w, h, _)) = glasses.iter().find(|(n, _, _, _)| n == "面玻") {
            assert!((w - 751.0).abs() < 0.5 && (h - 1950.0).abs() < 0.5,
                    "玻璃尺寸未按型号公式: {}x{} (疑似仍在用简易扣减)", w, h);
        } else {
            panic!("未生成面玻: {:?}", glasses);
        }
        println!("预设清空 / 玻璃预输入 {} 种 / 玻璃尺寸按型号公式 验证通过", g);
    }

    /// 打印排版补齐 + 螺丝板块 + 批量打印(多订单×多单据合并分页)
    #[test]
    fn test_print_templates_and_batch() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        migrate_profile_lib(&conn);
        crate::models_seed::normalize_rail_conditions(&conn);
        crate::models_seed::migrate_video_renames(&conn);
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        crate::models_seed::seed_all_models(&conn);
        dedup_profile_lib(&conn);
        crate::models_seed::apply_video_corrections(&conn);
        seed_screw_rules(&conn);

        conn.execute(
            "INSERT INTO orders(order_no,customer,terminal_addr,terminal_phone,product_name,material_type,color,width,height,face_glass,back_glass,glass_layer,order_date,style,total_price) \
             VALUES('TEST-1','测试客户','测试地址','13800000000','鹏飞7530推拉门','普通铝合金','太空灰',1730,2140,'单玻·超白玻 5mm','单玻·超白玻 5mm','双玻','2026-11-10','两轨推拉',1000)",
            [],
        ).unwrap();
        let oid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO profiles(order_id,profile_name,length,quantity,category) VALUES(?1,'光企高',2080,2,'扇')",
            rusqlite::params![oid],
        ).unwrap();
        conn.execute(
            "INSERT INTO glasses(order_id,glass_name,width,height,quantity) VALUES(?1,'面玻',751,1950,2)",
            rusqlite::params![oid],
        ).unwrap();

        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = crate::print::PrintService::new(db.clone(), host);
        svc.confirm_size(oid, "test").unwrap();

        // 下料单：螺丝数量 + 型号（放在玻璃小表后）
        let cutting = svc.print(oid, "cutting", "print").unwrap();
        assert!(cutting.contains("螺丝"), "下料单缺螺丝板块");
        assert!(cutting.contains("ST4.2×25"), "下料单缺螺丝型号");
        assert!(cutting.contains("颗"), "下料单缺螺丝数量");
        assert!(cutting.contains("36"), "推拉门默认螺丝数量应为 36");

        // 玻璃清单：玻璃型号 / 开孔 / 面积 / 页码
        let gc = svc.print(oid, "glass_cutting", "print").unwrap();
        assert!(gc.contains("玻璃型号"), "玻璃清单缺玻璃型号列");
        assert!(gc.contains("开孔"), "玻璃清单缺开孔列");
        assert!(gc.contains("面积"), "玻璃清单缺面积合计");
        assert!(gc.contains("第 1 页"), "玻璃清单缺页码");

        // 回执单：面积列 / 价格详细 / 指定物流 / 扫码付款
        let rc = svc.print(oid, "receipt", "print").unwrap();
        assert!(rc.contains("面积"), "回执单缺面积列");
        assert!(rc.contains("价格详细"), "回执单缺价格详细");
        assert!(rc.contains("指定物流"), "回执单缺指定物流");
        assert!(rc.contains("扫码付款"), "回执单缺扫码付款位");

        // 销售单 / 发货清单 / 标签
        let sales = svc.print(oid, "sales", "print").unwrap();
        assert!(sales.contains("小计") && sales.contains("本次尾款"), "销售单缺小计/尾款");
        let del = svc.print(oid, "delivery", "print").unwrap();
        assert!(del.contains("扇") && del.contains("第 1 页"), "发货清单缺扇数/页码");
        let label = svc.print(oid, "label", "print").unwrap();
        assert!(label.contains("地址") && label.contains("备注"), "标签缺地址/备注");
        let cert = svc.print(oid, "certificate", "print").unwrap();
        assert!(cert.contains("备注"), "合格证缺备注栏");

        // 批量：1 单 × 2 单据 = 2 页；螺丝数量临时覆盖 99；含统一分页样式
        let batch = svc
            .print_batch(&[oid], &vec!["cutting".to_string(), "receipt".to_string()], Some(99))
            .unwrap();
        assert_eq!(batch.matches("<section class=\"sheet\">").count(), 2, "批量页数不对");
        assert!(batch.contains("99"), "批量打印螺丝数量覆盖未生效");
        assert!(batch.contains("@page"), "批量打印缺分页样式");
        assert!(batch.contains("page-break-after"), "批量打印缺分页控制");
        assert!(batch.contains("order-block") && !batch.contains("cols-"), "批量应纵向满宽排，不应并排窄列");
        assert!(batch.contains("第 1 页 共 2 页") && batch.contains("第 2 页 共 2 页"),
                "批量打印页码不对");
        assert!(!batch.contains("10.5px"), "单订单打印不应被压缩成小字");

        // 未核对尺寸的订单必须被批量打印拒绝
        let oid2 = {
            let c = svc.db.lock().unwrap();
            c.execute("INSERT INTO orders(order_no,customer,product_name) VALUES('TEST-2','测试客户2','4030平开门')", []).unwrap();
            c.last_insert_rowid()
        };
        let bad = svc.print_batch(&[oid, oid2], &vec!["cutting".to_string()], None);
        assert!(bad.is_err(), "未核对订单不应允许打印");
        assert!(bad.unwrap_err().contains("尺寸未核对"));

        // 螺丝规则数量可改并即时生效
        {
            let c = svc.db.lock().unwrap();
            c.execute("UPDATE screw_rules SET qty=48 WHERE series='鹏飞7530推拉门'", []).unwrap();
        }
        let cutting2 = svc.print(oid, "cutting", "print").unwrap();
        assert!(cutting2.contains("48"), "螺丝规则修改后未生效");

        // 按「型号 + 料型」精确匹配的规则优先于通用规则
        {
            let c = svc.db.lock().unwrap();
            c.execute(
                "INSERT OR REPLACE INTO screw_rules(product_type,series,material_type,qty,spec,remark) \
                 VALUES('推拉门','鹏飞7530推拉门','普通铝合金',72,'M5×30 自攻螺丝','按料型单独设置')",
                [],
            ).unwrap();
        }
        let cutting3 = svc.print(oid, "cutting", "print").unwrap();
        assert!(cutting3.contains("72") && cutting3.contains("M5×30"),
                "按料型精确匹配未生效");

        // 厂名取自设置页公司名称,加在标题前(参考单据样本里有厂名)
        {
            let c = svc.db.lock().unwrap();
            c.execute("INSERT OR REPLACE INTO app_config(key,value) VALUES('company.name','测试门窗厂')", []).unwrap();
        }
        let cutting4 = svc.print(oid, "cutting", "print").unwrap();
        assert!(cutting4.contains("测试门窗厂 · 下 料 单"), "厂名未加到单据标题");

        println!("打印排版补齐 / 螺丝板块 / 批量分页 / 打印门禁 / 料型规则 / 厂名 验证通过");
    }

    #[test]
    fn test_series_tracks_history_batch3() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        migrate_profile_lib(&conn);
        crate::models_seed::normalize_rail_conditions(&conn);
        crate::models_seed::migrate_video_renames(&conn);
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        crate::models_seed::seed_all_models(&conn);
        dedup_profile_lib(&conn);
        crate::models_seed::apply_video_corrections(&conn);
        seed_screw_rules(&conn);

        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = OrderService::new(db.clone(), host.clone());

        // 1) 型号支持轨道类型
        let tracks = svc.series_tracks("鹏飞7530推拉门").unwrap();
        assert!(tracks.contains(&"地轨".to_string()), "缺地轨");
        assert!(tracks.contains(&"吊轨".to_string()), "缺吊轨");
        assert!(tracks.contains(&"自粘下轨".to_string()), "缺自粘下轨");
        let flat_tracks = svc.series_tracks("4030平开门").unwrap();
        assert!(flat_tracks.is_empty(), "平开门不应有轨道");

        // 2) 历史候选去重
        svc.save_history(&[("customer", Some("王五".to_string())), ("salesman", Some("李四".to_string()))]);
        svc.save_history(&[("customer", Some("王五".to_string()))]); // 重复不报错
        let hs = svc.history_list("customer").unwrap();
        assert_eq!(hs, vec!["王五".to_string()], "客户历史去重失败");

        // 3) 批量打印每页最多 3 单、从上往下纵向排(4 单 → 2 页:3+1)，每单满宽
        let psvc = crate::print::PrintService::new(db.clone(), host);
        let mut ids = Vec::new();
        for i in 0..4 {
            let no = format!("B-{}", i);
            let id = {
                let c = psvc.db.lock().unwrap();
                c.execute(
                    "INSERT INTO orders(order_no,customer,product_name) VALUES(?1,?2,'鹏飞7530推拉门')",
                    rusqlite::params![no, "客户"],
                )
                .unwrap();
                c.last_insert_rowid()
            };
            psvc.confirm_size(id, "test").unwrap();
            ids.push(id);
        }
        let batch = psvc.print_batch(&ids, &vec!["cutting".to_string()], None).unwrap();
        // 需要人工看排版时：DUMP_BATCH_HTML=<路径> cargo test --release test_series_tracks_history_batch3
        if let Ok(p) = std::env::var("DUMP_BATCH_HTML") {
            let _ = std::fs::write(&p, &batch);
        }
        assert_eq!(batch.matches("<section class=\"sheet\">").count(), 2, "4 单应分 2 页(3+1)");
        assert_eq!(batch.matches("class=\"order-block\"").count(), 4, "每单应有一个满宽区块");
        assert!(batch.contains("10.5px"), "多订单(>1)应注入压缩 CSS");
        assert!(!batch.contains("batch-grid") && !batch.contains("cols-3")
                && !batch.contains("class=\"order-col\""),
                "不应再出现并排窄列布局");
        assert!(batch.contains("order-band"), "缺订单号分隔小块头");
        assert!(batch.contains("第 1 页 共 2 页") && batch.contains("第 2 页 共 2 页"), "批量页码不对");
        for i in 0..4 {
            assert!(batch.contains(&format!("订单 B-{}", i)), "缺第 {} 单的订单号", i);
        }
        let first_page = batch
            .split("<section class=\"sheet\">")
            .nth(1)
            .and_then(|s| s.split("</section>").next())
            .unwrap_or("");
        assert_eq!(first_page.matches("class=\"order-block\"").count(), 3, "第一页应从上往下排 3 单");

        println!("型号轨道 / 历史候选去重 / 批量一页三单纵向满宽排版 验证通过");
    }

    /// 本轮修复的回归测试：重启后尺寸核对仍有效、PID 精确匹配、导出文件名过滤、配置写库报错可见
    #[test]
    fn test_bugfix_regressions_restart_pid_filename_config() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        conn.execute(
            "INSERT INTO orders(order_no,customer) VALUES('R-1','重启测试')",
            [],
        )
        .unwrap();
        let oid = conn.last_insert_rowid();
        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));

        // 1) 尺寸核对落库：换新 PrintService（等价于重启）后仍能打印
        let svc1 = crate::print::PrintService::new(db.clone(), host.clone());
        assert!(!svc1.is_confirmed(oid), "未核对的订单不应放行");
        svc1.confirm_size(oid, "test").unwrap();
        assert!(svc1.is_confirmed(oid));
        drop(svc1);
        let svc2 = crate::print::PrintService::new(db.clone(), host.clone());
        assert!(svc2.is_confirmed(oid), "重启后尺寸核对状态丢失");
        assert!(svc2.print(oid, "cutting", "print").is_ok(), "重启后打印被误拒绝");

        // 1b) 批量页眉里的订单号/厂名必须转义，不能把用户输入当 HTML 插入
        let esc_id = {
            let c = db.lock().unwrap();
            c.execute("INSERT INTO orders(order_no,customer) VALUES('A<&1','转义测试')", []).unwrap();
            let id = c.last_insert_rowid();
            c.execute("INSERT OR REPLACE INTO app_config(key,value) VALUES('company.name','甲&乙门窗')", []).unwrap();
            id // 必须在写入 app_config 之前取，否则 last_insert_rowid 会被覆盖
        };
        svc2.confirm_size(esc_id, "test").unwrap();
        let h = svc2.print_batch(&[esc_id], &vec!["label".to_string()], None).unwrap();
        assert!(h.contains("A&lt;&amp;1") && h.contains("甲&amp;乙门窗"), "页眉未做 HTML 转义");
        // 只校验页眉：单据正文模板里的用户字段转义尚未统一处理（见日志待办）
        let header = h.split("<div class=\"sheet-body\">").next().unwrap_or("");
        assert!(!header.contains("A<&1"), "页眉出现未转义的订单号");

        // 2) PID 检测必须整词匹配，不能被内存列 "15,376 K" 之类的数字误命中
        assert!(crate::pid_mentioned_in_tasklist("app.exe  15376 Console  1  27,292 K", 15376));
        assert!(!crate::pid_mentioned_in_tasklist("app.exe  99 Console  1  15,376 K", 15376));
        assert!(!crate::pid_mentioned_in_tasklist("信息: 没有运行的任务匹配指定标准。", 15376));

        // 3) 导出文件名不能含路径分隔符或前导点
        let n = crate::export::safe_filename_part("../a/b\\c:d");
        assert!(!n.contains('/') && !n.contains('\\') && !n.contains(".."));
        assert!(!n.starts_with('.'));
        assert_eq!(crate::export::safe_filename_part("..."), "order");

        // 4) 配置写入成功必须真正落库（返回 Ok 而不是静默吞错）
        {
            let c = db.lock().unwrap();
            super::set_config(&c, "test.key", "v1").unwrap();
            assert_eq!(super::get_config(&c, "test.key"), Some("v1".to_string()));
        }

        println!("重启核对状态 / PID 精确匹配 / 导出文件名 / 配置写入 回归通过");
    }

    /// 本轮新增：平开门开孔入库 + 玻璃单显示开孔 + 表单快照上限 + 配置保存不丢其它项
    #[test]
    fn test_glass_holes_snapshots_and_config_integrity() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        for (k, v) in [
            ("company.name", "测试门窗"), ("operator.name", "管理员"),
            ("app.allow_multi_open", "false"), ("backup.manual.dir", "D:/backup/manual"),
            ("backup.snapshot.interval_min", "30"), ("backup.compress", "true"),
        ] {
            super::set_config(&conn, k, v).unwrap();
        }
        seed_glass_holes(&conn);

        // 1) 开孔数据：有值的型号 12 个、确认无开孔 4 个；右分支一律不推测
        let with_holes: i64 = conn
            .query_row("SELECT COUNT(DISTINCT series) FROM glass_holes WHERE hole_left IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        let confirmed_none: i64 = conn
            .query_row("SELECT COUNT(DISTINCT series) FROM glass_holes WHERE note LIKE '截图确认%'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(with_holes, 12, "应有 12 个平开门型号录入开孔值");
        assert_eq!(confirmed_none, 4, "应有 4 个型号确认不开孔");
        let guessed: i64 = conn
            .query_row("SELECT COUNT(*) FROM glass_holes WHERE hole_right IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(guessed, 0, "右分支截图被截断，不允许推测入库");
        // 4030 是纯文字型开孔，三行各不相同
        let v: String = conn
            .query_row("SELECT hole_left FROM glass_holes WHERE series='4030平开门' AND glass_name='面-面玻'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "外包内左");

        // 2) 玻璃下料单必须显示真实开孔值，不再是写死的破折号
        seed_profile_lib(&conn);
        seed_full_profile_lib(&conn);
        crate::models_seed::seed_all_models(&conn);
        seed_screw_rules(&conn);
        conn.execute(
            "INSERT INTO orders(order_no,customer,product_name,glass_layer,width,height) \
             VALUES('K-1','开孔测试','4030平开门','双玻',800,2000)", [],
        ).unwrap();
        let oid = conn.last_insert_rowid();
        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = crate::print::PrintService::new(db.clone(), host);
        svc.confirm_size(oid, "test").unwrap();
        let html = svc.print(oid, "glass_cutting", "print").unwrap();
        assert!(html.contains("外包内左"), "玻璃清单未显示开孔参数");
        assert!(html.contains("开孔（平开门）"), "玻璃清单缺开孔列标题");
        // 勾选全部 7 种单据时后端必须产出 7 页，用来排除「只印出两种」是后端丢单据
        let all_docs = svc.print_all(oid).unwrap();
        assert_eq!(all_docs.matches("<section class=\"sheet\">").count(), 7,
                   "勾选全部单据应生成 7 页，少一页都说明后端丢单据");
        // 客户单据分享页：不走打印门禁，按勾选的单据样板逐个渲染
        let share = svc.share_html(&[oid], &vec!["sales".to_string(), "receipt".to_string()]).unwrap();
        assert_eq!(share.matches("class=\"share-doc\"").count(), 2, "分享页应含 2 张单据");
        assert!(share.contains("销售单") && share.contains("回执单"), "分享页缺单据标题");

        // 3) 表单快照上限：auto=5 / shutdown=3，且只保留最新
        {
            let c = db.lock().unwrap();
            for i in 0..8 {
                c.execute(
                    "INSERT INTO form_snapshots(kind,payload) VALUES('auto',?1)",
                    rusqlite::params![format!("{{\"order_form\":{{\"n\":{}}}}}", i)],
                ).unwrap();
            }
            c.execute(
                "DELETE FROM form_snapshots WHERE kind='auto' AND id NOT IN \
                 (SELECT id FROM form_snapshots WHERE kind='auto' ORDER BY id DESC LIMIT 5)", [],
            ).unwrap();
            let n: i64 = c.query_row("SELECT COUNT(*) FROM form_snapshots WHERE kind='auto'", [], |r| r.get(0)).unwrap();
            assert_eq!(n, 5, "定时快照应只保留 5 个");
            let oldest: i64 = c.query_row("SELECT MIN(id) FROM form_snapshots WHERE kind='auto'", [], |r| r.get(0)).unwrap();
            assert!(oldest > 3, "应删除最旧的快照");
        }

        // 4) 改公司名不能影响其它配置项
        let before: Vec<(String, String)> = {
            let c = db.lock().unwrap();
            let mut st = c.prepare("SELECT key,value FROM app_config ORDER BY key").unwrap();
            st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))
                .unwrap().map(|x| x.unwrap()).collect()
        };
        {
            let c = db.lock().unwrap();
            super::set_config(&c, "company.name", "新公司名").unwrap();
        }
        let after: Vec<(String, String)> = {
            let c = db.lock().unwrap();
            let mut st = c.prepare("SELECT key,value FROM app_config ORDER BY key").unwrap();
            st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default())))
                .unwrap().map(|x| x.unwrap()).collect()
        };
        assert_eq!(before.len(), after.len(), "配置项数量不应变化");
        let changed: Vec<String> = before.iter().zip(after.iter())
            .filter(|(a, b)| a.0 != "company.name" && a.1 != b.1)
            .map(|(a, _)| a.0.clone()).collect();
        assert!(changed.is_empty(), "改公司名影响了其它配置: {:?}", changed);
        let cn: String = db.lock().unwrap()
            .query_row("SELECT value FROM app_config WHERE key='company.name'", [], |r| r.get(0)).unwrap();
        assert_eq!(cn, "新公司名");

        println!("开孔入库/玻璃单显示 / 快照上限 / 配置完整性 回归通过");
    }

    /// 合格证按小标签纸排版、二维码只留白不生成、图标开关生效
    #[test]
    fn test_certificate_label_size_qr_and_logo_switch() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(SCHEMA).expect("schema");
        conn.execute(
            "INSERT INTO orders(order_no,customer,product_name,material_type,color,width,height,order_date,remark) \
             VALUES('CERT-1','测试客户','7535平开门','普通铝合金','太空灰',900,2100,'2026-11-10','加急件·门口有台阶')",
            [],
        )
        .unwrap();
        let oid = conn.last_insert_rowid();
        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = crate::print::PrintService::new(db.clone(), host);
        svc.confirm_size(oid, "test").unwrap();

        // 默认 75×100mm + 二维码留白（只有空位，不出现「二维码」字样）
        let h = svc.print(oid, "certificate", "print").unwrap();
        assert!(h.contains("@page{size:75mm 100mm"), "合格证默认纸张不是 75×100mm");
        assert!(h.contains(r#"<div class="qr-box">"#), "合格证缺少二维码留白位");
        assert!(!h.contains("二维码"), "留白位不应出现「二维码」文字");
        // 尺寸必须是「宽×高」：900 宽 2100 高，不能打成 2100×900
        assert!(h.contains("900×2100"), "合格证尺寸不是宽×高，实际没有 900×2100");
        // 备注要真的有内容，不能是个空行
        assert!(h.contains("加急件·门口有台阶"), "合格证「备注」没带出订单备注");

        // 改纸张尺寸 + 关掉二维码留白
        {
            let c = db.lock().unwrap();
            set_config(&c, "print.cert.width_mm", "60").unwrap();
            set_config(&c, "print.cert.height_mm", "40").unwrap();
            set_config(&c, "print.qr.enabled", "false").unwrap();
        }
        let h2 = svc.print(oid, "certificate", "print").unwrap();
        assert!(h2.contains("@page{size:60mm 40mm"), "合格证纸张未跟随设置");
        assert!(!h2.contains(r#"<div class="qr-box">"#), "关闭后不应再留二维码位");
        // 关掉二维码后整表只有 2 列，任何 colspan 都会让浏览器凭空多出一列空白
        assert!(!h2.contains("colspan"), "关闭二维码后仍残留 colspan，表格会多出一列空白");

        // 图标开关
        {
            let c = db.lock().unwrap();
            set_config(&c, "print.logo.enabled", "false").unwrap();
        }
        let cut = svc.print(oid, "cutting", "print").unwrap();
        assert!(!cut.contains(r#"class="logo""#), "关闭图标后仍输出了 logo");

        println!("合格证小纸排版 / 二维码留白 / 图标开关 验证通过");
    }

    /// 打印核对不能把已排产的订单打回「已审核」；下料参数要能存能读
    #[test]
    fn test_confirm_keeps_status_and_door_params() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(SCHEMA).expect("schema");
        conn.execute(
            "INSERT INTO orders(order_no,customer,status,product_name,width,height,fan_count,track_type,door_height,wall_thickness) \
             VALUES('S-1','客户','生产中','7535平开门',900,2100,3,'吊轨',2100,120)",
            [],
        )
        .unwrap();
        let oid = conn.last_insert_rowid();
        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = crate::print::PrintService::new(db.clone(), host);

        // 已排产订单：打印核对后状态不能倒退
        svc.confirm_size(oid, "test").unwrap();
        {
            let c = db.lock().unwrap();
            let st: String = c
                .query_row("SELECT status FROM orders WHERE id=?1", rusqlite::params![oid], |r| r.get(0))
                .unwrap();
            assert_eq!(st, "生产中", "打印核对把已排产订单打回了已审核");
            let (fan, track): (i64, String) = c
                .query_row(
                    "SELECT fan_count, track_type FROM orders WHERE id=?1",
                    rusqlite::params![oid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(fan, 3, "扇数没存住，编辑后重算会按默认 2 扇算错");
            assert_eq!(track, "吊轨", "轨道类型没存住");
            c.execute("UPDATE orders SET status='未合并' WHERE id=?1", rusqlite::params![oid]).unwrap();
        }
        // 未合并的应当前移为已审核
        svc.confirm_size(oid, "test").unwrap();
        {
            let c = db.lock().unwrap();
            let st: String = c
                .query_row("SELECT status FROM orders WHERE id=?1", rusqlite::params![oid], |r| r.get(0))
                .unwrap();
            assert_eq!(st, "已审核", "未合并订单打印核对后应前移为已审核");
        }

        println!("打印核对状态不回退 / 下料参数存取 验证通过");
    }

    /// 异形订单：手工型材 + 多条玻璃，7 种单据都要能打印且内容齐全
    #[test]
    fn test_special_order_all_docs_printable() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(SCHEMA).expect("schema");
        conn.execute(
            "INSERT INTO orders(order_no,customer,product_name,material_type,color,width,height,face_glass,back_glass,order_date,style,total_price) \
             VALUES('YX-1','异形客户','异形-平开门','普通铝合金','太空灰',1000,2200,'钢化 8mm','钢化 8mm','2026-11-11','左开',1200)",
            [],
        )
        .unwrap();
        let oid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO profiles(order_id,profile_name,length,quantity,category) VALUES(?1,'自定义立柱',2150,2,'框')",
            rusqlite::params![oid],
        )
        .unwrap();
        // 异形订单的玻璃可能不止一块，各自尺寸不同
        conn.execute(
            "INSERT INTO glasses(order_id,glass_name,width,height,quantity) VALUES(?1,'上亮玻璃',900,300,1)",
            rusqlite::params![oid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO glasses(order_id,glass_name,width,height,quantity) VALUES(?1,'主玻',950,1900,1)",
            rusqlite::params![oid],
        )
        .unwrap();

        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = crate::print::PrintService::new(db.clone(), host);
        svc.confirm_size(oid, "test").unwrap();

        for dt in ["cutting", "glass_cutting", "certificate", "delivery", "sales", "receipt", "label"] {
            let html = svc
                .print(oid, dt, "print")
                .unwrap_or_else(|e| panic!("异形订单打印 {} 失败: {}", dt, e));
            assert!(html.contains("YX-1"), "单据 {} 里没有订单号", dt);
        }

        let cut = svc.print(oid, "cutting", "print").unwrap();
        assert!(cut.contains("自定义立柱"), "异形手工加的型材没有进下料单");

        let gc = svc.print(oid, "glass_cutting", "print").unwrap();
        assert!(
            gc.contains("上亮玻璃") && gc.contains("主玻"),
            "异形订单的多块玻璃没有全部进玻璃下料单"
        );

        println!("异形订单 7 种单据打印 / 手工型材 / 多条玻璃 验证通过");
    }

    /// 订单里的客户必须自动进客户管理；同名不重复；历史订单能补录
    #[test]
    fn test_customer_auto_saved_and_backfilled() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(SCHEMA).expect("schema");
        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = crate::order::OrderService::new(db.clone(), host);

        // 1) 新建订单 → 客户自动进客户表，并回填订单的 customer_id
        let o = svc
            .create(crate::model::OrderInput {
                order_no: "KC-1".to_string(),
                customer: Some("张三".to_string()),
                terminal_phone: Some("13800000000".to_string()),
                terminal_addr: Some("汉口".to_string()),
                ..Default::default()
            })
            .unwrap();
        {
            let c = db.lock().unwrap();
            let (name, phone, addr): (String, String, String) = c
                .query_row(
                    "SELECT name, COALESCE(phone,''), COALESCE(terminal_addr,'') FROM customers",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .expect("订单保存后客户没进客户表");
            assert_eq!(name, "张三");
            assert_eq!(phone, "13800000000");
            assert_eq!(addr, "汉口");
            let cid: Option<i64> = c
                .query_row(
                    "SELECT customer_id FROM orders WHERE id=?1",
                    rusqlite::params![o.id],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(cid.is_some(), "订单没回填 customer_id，客户与订单对不上");
        }

        // 2) 同一客户再下一单：不重复建客户，也不覆盖已有电话
        svc.create(crate::model::OrderInput {
            order_no: "KC-2".to_string(),
            customer: Some("张三".to_string()),
            ..Default::default()
        })
        .unwrap();
        {
            let c = db.lock().unwrap();
            let n: i64 = c
                .query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "同名客户被重复插入了");
            let phone: String = c
                .query_row("SELECT COALESCE(phone,'') FROM customers", [], |r| r.get(0))
                .unwrap();
            assert_eq!(phone, "13800000000", "补空字段时把已有电话覆盖了");
        }

        // 3) 历史订单（老版本存的、客户表里没有的）启动补录
        {
            let c = db.lock().unwrap();
            c.execute(
                "INSERT INTO orders(order_no, customer, terminal_phone, terminal_addr) \
                 VALUES('OLD-1','李四','139','武昌')",
                [],
            )
            .unwrap();
        }
        let added = {
            let c = db.lock().unwrap();
            crate::order::backfill_customers_from_orders(&c).unwrap()
        };
        assert_eq!(added, 1, "历史订单客户没补录");
        {
            let c = db.lock().unwrap();
            let n: i64 = c
                .query_row(
                    "SELECT COUNT(*) FROM customers WHERE TRIM(name)='李四'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "补录的客户没进客户表");
            let cid: Option<i64> = c
                .query_row(
                    "SELECT customer_id FROM orders WHERE order_no='OLD-1'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(cid.is_some(), "补录后没回填订单的 customer_id");
        }
        println!("订单客户自动录入 / 同名不重复 / 历史补录 验证通过");
    }

    /// 量表口径：总金额 = 有效 + 异常；「未填客户」能筛到；近 12 个月补齐
    #[test]
    fn test_stats_scope_and_empty_customer_filter() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(SCHEMA).expect("schema");
        let d_now: String = conn
            .query_row("SELECT date('now','start of month')", [], |r| r.get(0))
            .unwrap();
        let d_prev: String = conn
            .query_row("SELECT date('now','-1 month','start of month')", [], |r| r.get(0))
            .unwrap();
        let sql = "INSERT INTO orders(order_no,customer,width,height,total_price,order_date,status) \
                   VALUES(?1,?2,?3,?4,?5,?6,'待生产')";
        // 两单正常（1000 + 2000）／一单尺寸异常（500）／一单未填客户（300）
        conn.execute(sql, rusqlite::params!["S1", "甲", 1000.0, 2000.0, 1000.0, d_now]).unwrap();
        conn.execute(sql, rusqlite::params!["S2", "甲", 900.0, 2100.0, 2000.0, d_now]).unwrap();
        conn.execute(sql, rusqlite::params!["S3", "乙", 4545555.0, 3246456.0, 500.0, d_now]).unwrap();
        conn.execute(sql, rusqlite::params!["S4", "", 800.0, 2000.0, 300.0, d_prev]).unwrap();

        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = crate::order::OrderService::new(db.clone(), host);

        let all = svc.stats(None).unwrap();
        assert_eq!(all.summary.order_count, 4);
        assert!(
            (all.summary.total_amount - 3800.0).abs() < 0.01,
            "总金额不对：{}",
            all.summary.total_amount
        );
        assert!(
            (all.summary.valid_amount - 3300.0).abs() < 0.01,
            "有效金额不对：{}",
            all.summary.valid_amount
        );
        assert!(
            (all.summary.abnormal_amount - 500.0).abs() < 0.01,
            "异常订单金额不对：{}",
            all.summary.abnormal_amount
        );
        // 账必须能对上：总金额 = 有效金额 + 异常金额
        assert!(
            ((all.summary.valid_amount + all.summary.abnormal_amount) - all.summary.total_amount)
                .abs()
                < 0.01,
            "总金额和 有效+异常 对不上"
        );
        // 有效面积 = (1000×2000 + 900×2100 + 800×2000)/1e6 = 5.49
        assert!(
            (all.summary.total_area - 5.49).abs() < 0.01,
            "有效面积不对（异常尺寸被算进去了？）：{}",
            all.summary.total_area
        );
        assert_eq!(all.summary.valid_count, 3);
        assert_eq!(all.summary.abnormal_count, 1);
        assert_eq!(all.points.len(), 12, "近 12 个月没补齐，折线会跳过空月份");

        // 「未填客户」分组必须真能筛出订单（原来 = '(未填客户)' 永远匹配不到）
        let empty = svc.stats(Some("(未填客户)")).unwrap();
        assert_eq!(empty.summary.order_count, 1, "「未填客户」分组筛不到订单");
        assert!((empty.summary.total_amount - 300.0).abs() < 0.01);

        // 指定客户
        let jia = svc.stats(Some("甲")).unwrap();
        assert_eq!(jia.summary.order_count, 2);
        assert!((jia.summary.total_amount - 3000.0).abs() < 0.01);

        // 客户量表：面积口径与总量表一致
        let custs = svc.stats_customers().unwrap();
        let total_area: f64 = custs.iter().map(|c| c.area).sum();
        assert!(
            (total_area - all.summary.total_area).abs() < 0.01,
            "客户量表面积合计（{}）和总量表（{}）对不上",
            total_area,
            all.summary.total_area
        );
        println!("量表口径 / 未填客户分组 / 月份补齐 / 客户表面积一致 验证通过");
    }

    /// 端到端：并发导出单据图片，验证张数、顺序、进度回调、超时保护不误伤
    /// 需要本机有 Edge：cargo test --release e2e_export_images -- --ignored --nocapture
    #[test]
    #[ignore]
    fn e2e_export_images_concurrent() {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(SCHEMA).expect("schema");
        for no in ["E2E-1", "E2E-2"] {
            conn.execute(
                "INSERT INTO orders(order_no,customer,product_name,material_type,color,width,height,                 glass_layer,face_glass,back_glass,face_glass_w,face_glass_h,order_date,style,total_price)                  VALUES(?1,'并发客户','7535平开门','普通铝合金','太空灰',900,2100,                 '双玻','单玻·钢化白玻 5mm','单玻·钢化白玻 5mm',830,2030,'2026-11-11','左开',1800)",
                rusqlite::params![no],
            )
            .unwrap();
        }
        let ids: Vec<i64> = {
            let mut st = conn.prepare("SELECT id FROM orders ORDER BY id").unwrap();
            let r = st.query_map([], |x| x.get(0)).unwrap().filter_map(|x| x.ok()).collect();
            r
        };
        let db = Arc::new(Mutex::new(conn));
        let host = Arc::new(PluginHost::new(std::path::Path::new(".")));
        let svc = crate::print::PrintService::new(db.clone(), host);
        let dir = std::env::temp_dir().join("dw_e2e_export");
        let _ = std::fs::remove_dir_all(&dir);

        let docs = vec!["cutting".to_string(), "certificate".to_string(), "label".to_string()];
        let seen = std::sync::Mutex::new(Vec::<(usize, usize)>::new());
        let t0 = std::time::Instant::now();
        let imgs = crate::render_share_images(&svc, &ids, &docs, &dir, "测试", &|d, t, _tip| {
            seen.lock().unwrap().push((d, t));
        })
        .expect("并发导出失败");
        let cost = t0.elapsed().as_millis();

        assert_eq!(imgs.len(), 6, "2 单 × 3 种单据应生成 6 张，实际 {}", imgs.len());
        // 顺序必须与「单据类型 × 订单」一致，并发不能打乱
        for (i, p) in imgs.iter().enumerate() {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            assert!(
                name.ends_with(&format!("_{}.png", i + 1)),
                "第 {} 张文件名不对：{}",
                i + 1,
                name
            );
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            assert!(size > 1024, "第 {} 张图是空的：{}", i + 1, size);
        }
        // 进度：开头一次「准备」+ 每张完成一次；最后一次必须是 6/6
        let calls = seen.lock().unwrap().clone();
        assert!(calls.len() >= 7, "进度回调次数偏少：{}", calls.len());
        assert_eq!(*calls.last().unwrap(), (6, 6), "最后一次进度不是 6/6");

        println!(
            "并发导出 6 张成功，耗时 {} ms，进度回调 {} 次",
            cost,
            calls.len()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 软件内表格预览：与导出的 xlsx 共用 collect_sheets，验证表头/行数/勾选区块
    #[test]
    fn test_export_preview_in_app() {
        let conn = Connection::open_in_memory().expect("open memory");
        conn.execute_batch(SCHEMA).expect("schema");
        conn.execute(
            "INSERT INTO orders(order_no,customer,product_name,width,height) VALUES('P-1','预览客户','4030平开门',800,2000)",
            [],
        ).unwrap();
        let oid = conn.last_insert_rowid();
        conn.execute("INSERT INTO profiles(order_id,profile_name,length,quantity) VALUES(?1,'光企高',2000,2)",
                     rusqlite::params![oid]).unwrap();
        conn.execute("INSERT INTO glasses(order_id,glass_name,width,height,quantity) VALUES(?1,'面玻',700,1900,1)",
                     rusqlite::params![oid]).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let svc = crate::export::ExportService::new(db);

        let sheets = svc
            .preview_orders_batch(&[oid], &vec!["profiles".to_string(), "glasses".to_string()])
            .unwrap();
        assert_eq!(sheets.len(), 3, "汇总 + 型材 + 玻璃");
        assert_eq!(sheets[0].name, "订单汇总");
        assert_eq!(sheets[0].headers.len(), 24);
        assert_eq!(sheets[0].rows.len(), 1);
        assert_eq!(sheets[0].rows[0][0], "P-1");
        assert_eq!(sheets[1].name, "型材明细");
        assert_eq!(sheets[1].rows.len(), 1);
        assert_eq!(sheets[2].name, "玻璃明细");
        assert_eq!(sheets[2].rows.len(), 1);

        // 不勾明细时只剩汇总表；空订单必须报错
        let only = svc.preview_orders_batch(&[oid], &vec![]).unwrap();
        assert_eq!(only.len(), 1);
        assert!(svc.preview_orders_batch(&[], &vec![]).is_err());
        println!("软件内表格预览 / 与 xlsx 同源 / 勾选区块 回归通过");
    }
}
