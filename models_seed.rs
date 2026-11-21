// src-tauri/src/models_seed.rs
// 按用户提供的「型号分类表」补全全部型号的型材/算法变量/玻璃公式。
// 数据来源:用户提供的移门管理公式管理界面截图 + 读图工作日志(2026-11-08)。
// 统一骨架(行业通用下料结构) + 每型号实测参数;截图未覆盖的型号按同系列推定,
// 最终来源清单见 D:\hhhhh\型号公式补全清单.xlsx。

use rusqlite::{params, Connection};
use std::collections::HashSet;

/// 型材行:(系列, 名称, 公式, 数量, 分类, 条件, 数量公式)
type PRow = (String, String, String, i64, String, String, String);
/// 玻璃行:(系列, 名称, 高公式, 宽公式, 数量公式, 条件)
type GRow = (String, String, String, String, String, String);

#[derive(Clone)]
struct Slide {
    series: &'static str,
    /// 光企高算法:(轨道类型, 扣减量),公式 = base - 扣减量
    rail: &'static [(&'static str, i64)],
    /// 上下方算法变量:(条件, 公式)
    sxf: &'static [(&'static str, &'static str)],
    base: &'static str, // 默认 "含轨道的高度"
    glass_h: i64,       // 面玻高 = 光企高算法 - glass_h
    glass_w: &'static str, // 面玻宽完整公式
    win_h: &'static str,   // 上亮玻璃高完整公式
    edge: &'static str,
    track: &'static str,
    case_h: &'static str,
    case_w: &'static str,
    win_w: &'static [&'static str], // 窗数1..4,空则用默认
    single: bool,                    // 单玻型号:不生成背玻
    src: &'static str,               // 公式来源标记
}

#[derive(Clone)]
struct Flat {
    series: &'static str,
    outer_h: &'static str, // 外框高
    door_h: &'static str,  // 门扇高(=门框高)
    door_w: &'static str,  // 门扇宽(=门框宽)
    glass_cut: i64,        // 玻璃扣减
    src: &'static str,
}

/// static 初始化必须全为常量表达式,不能用 Default::default()
const EMPTY_SLIDE: Slide = Slide {
    series: "", rail: &[], sxf: &[], base: "", glass_h: 0, glass_w: "", win_h: "",
    edge: "", track: "", case_h: "", case_w: "", win_w: &[], single: false, src: "",
};
const EMPTY_FLAT: Flat = Flat {
    series: "", outer_h: "", door_h: "", door_w: "", glass_cut: 0, src: "",
};

fn pr(s: &str, n: &str, f: &str, q: i64, c: &str, cond: &str, qf: &str) -> PRow {
    (s.into(), n.into(), f.into(), q, c.into(), cond.into(), qf.into())
}
fn gr(s: &str, n: &str, h: &str, w: &str, q: &str, cond: &str) -> GRow {
    (s.into(), n.into(), h.into(), w.into(), q.into(), cond.into())
}

const DEFAULT_WIN_W: [&str; 4] = [
    "(总宽-30-30)",
    "(总宽-30-30-30)/2",
    "(总宽-30-30-30-30)/3",
    "(总宽-30-30-30-30-30)/4",
];

/// 录屏 QQ20261109-125606.mp4 确认:打开这些型号时软件弹出
/// 「要查询的公式为空! 您要继续上面这个公式吗?」。
/// 此时界面上显示的其实是「上一个型号」的残留公式,不能录入。
/// 用户规则:**保留型号名,但不录入任何公式**。
/// 末尾两个是原软件里本来就没内容的占位型号。
pub const EMPTY_MODELS: &[&str] = &[
    "1635推拉门吊轨鹏飞",    // t=160 弹窗确认
    "1635四连动",            // t=181 弹窗确认
    "1635单玻三联动推拉门",  // t=217 弹窗确认
    "1645单玻三联动推拉门",  // t=228 弹窗确认
    "新增产品名称16",        // 原软件占位项,公式为空
    "新增产品名称25",        // 原软件占位项,公式为空
];

pub fn is_empty_model(s: &str) -> bool {
    EMPTY_MODELS.iter().any(|x| *x == s)
}

/// 生成一个推拉门型号的完整公式集(型材 + 算法变量 + 玻璃)
fn build_slide(m: &Slide) -> (Vec<PRow>, Vec<GRow>) {
    let s = m.series;
    let base = if m.base.is_empty() { "含轨道的高度" } else { m.base };
    let edge = if m.edge.is_empty() { "含轨道的高度-1020" } else { m.edge };
    let track = if m.track.is_empty() { "总宽-40-1020" } else { m.track };
    let case_h = if m.case_h.is_empty() { "总高-28+36-510" } else { m.case_h };
    let case_w = if m.case_w.is_empty() { "总宽-40+71" } else { m.case_w };
    let win_h = if m.win_h.is_empty() { "上亮高-60+5" } else { m.win_h };
    let glass_w = if m.glass_w.is_empty() { "上下方算法变量-23-28" } else { m.glass_w };
    let ww: Vec<&str> = if m.win_w.is_empty() {
        DEFAULT_WIN_W.to_vec()
    } else {
        m.win_w.to_vec()
    };

    let mut p = Vec::new();
    // ---------- 型材(框/扇) ----------
    p.push(pr(s, "边封高", edge, 2, "框", "", ""));
    p.push(pr(s, "轨道长", track, 1, "框", "", ""));
    p.push(pr(s, "光企高", "光企高算法", 1, "扇", "", "光企数量"));
    p.push(pr(s, "勾企高", "光企高算法", 1, "扇", "", "勾企数量"));
    p.push(pr(s, "上下方", "上下方算法变量", 1, "扇", "", "扇数*2"));
    p.push(pr(s, "收口", "光企高算法", 1, "框", "扇数=4 or 是否有收口=1", ""));
    p.push(pr(s, "上亮宽", "总宽", 2, "框", "门高>0", ""));
    p.push(pr(s, "套线高", case_h, 2, "框", "包套=双包", ""));
    p.push(pr(s, "套线宽", case_w, 1, "框", "包套=双包", ""));
    p.push(pr(s, "铝板高", "总高-50", 2, "框", "墙厚>0", ""));
    p.push(pr(s, "铝板宽", "总宽-50", 1, "框", "墙厚>0", ""));
    p.push(pr(s, "F条高", "总高-50", 2, "框", "墙厚>0", ""));
    p.push(pr(s, "F条宽", "总宽-50", 1, "框", "墙厚>0", ""));
    p.push(pr(s, "铝板厚", "墙厚-50", 1, "框", "墙厚>0", ""));

    // ---------- 算法变量 ----------
    p.push(pr(s, "含轨道的高度", "总高", 1, "变量", "门高<=0", ""));
    p.push(pr(s, "含轨道的高度", "门高", 1, "变量", "门高>0", ""));
    for (rail, cut) in m.rail {
        p.push(pr(s, "光企高算法", &format!("{}-{}", base, cut), 1, "变量",
                 &format!("轨道类型={}", rail), ""));
    }
    for (cond, f) in m.sxf {
        p.push(pr(s, "上下方算法变量", f, 1, "变量", cond, ""));
    }
    // 光企/勾企数量(按扇数 + 重叠边数量)
    for (name, cond, v) in [
        ("光企数量", "扇数=2", "2"),
        ("光企数量", "扇数=3 and 重叠边数量=1", "4"),
        ("光企数量", "扇数=3 and 重叠边数量=2", "2"),
        ("光企数量", "扇数=4", "4"),
        ("勾企数量", "扇数=2", "2"),
        ("勾企数量", "扇数=3 and 重叠边数量=1", "4"),
        ("勾企数量", "扇数=3 and 重叠边数量=2", "2"),
        ("勾企数量", "扇数=4", "4"),
    ] {
        p.push(pr(s, name, v, 1, "变量", cond, ""));
    }
    p.push(pr(s, "是否有收口", "1", 1, "变量", "重叠边数量=2 and 扇数=3", ""));
    p.push(pr(s, "是否有收口", "0", 1, "变量", "重叠边数量!=2", ""));
    p.push(pr(s, "是否有收口", "0", 1, "变量", "扇数!=3", ""));
    p.push(pr(s, "套线数量变量", "2", 1, "变量", "包套=双包", ""));
    p.push(pr(s, "套线数量变量", "1", 1, "变量", "包套!=双包", ""));
    for (i, f) in ww.iter().enumerate() {
        p.push(pr(s, "上亮玻璃宽", f, 1, "变量", &format!("窗数={}", i + 1), ""));
        p.push(pr(s, "上亮大玻璃数量", &format!("{}", i + 1), 1, "变量",
                 &format!("窗数={}", i + 1), ""));
    }
    p.push(pr(s, "上亮高", "总高-门高", 1, "变量", "门高>0", ""));

    // ---------- 玻璃 ----------
    let mut g = Vec::new();
    let gh = format!("光企高算法-{}", m.glass_h);
    g.push(gr(s, "面玻", &gh, glass_w, "扇数", ""));
    if !m.single {
        g.push(gr(s, "背玻", &gh, glass_w, "扇数", "单双玻=双玻"));
    }
    g.push(gr(s, "窗-窗面", win_h, "上亮玻璃宽", "上亮大玻璃数量", "门高>0"));
    g.push(gr(s, "窗-窗背", win_h, "上亮玻璃宽", "上亮大玻璃数量", "门高>0"));
    (p, g)
}

/// 生成一个平开门型号的完整公式集
fn build_flat(m: &Flat) -> (Vec<PRow>, Vec<GRow>) {
    let s = m.series;
    let oh = if m.outer_h.is_empty() { "总高-510" } else { m.outer_h };
    let dh = if m.door_h.is_empty() { "总高-30" } else { m.door_h };
    let dw = if m.door_w.is_empty() { "总宽-42" } else { m.door_w };
    // 注意:「上亮宽」只保留为算法变量,不生成同名框料行,
    // 否则会被 dedup_profile_lib 当作「误标为框的变量行」清掉。
    let p = vec![
        pr(s, "外框高", oh, 2, "框", "", ""),
        pr(s, "外框宽", "总宽", 1, "框", "", ""),
        pr(s, "门框高", dh, 2, "框", "", ""),
        pr(s, "门框宽", dw, 2, "框", "", ""),
        pr(s, "门扇高", dh, 1, "变量", "", ""),
        pr(s, "门扇宽", dw, 1, "变量", "", ""),
        pr(s, "上亮宽", "总宽", 1, "变量", "窗高>0", ""),
    ];
    let gh = format!("门扇高-{}", m.glass_cut);
    let gw = format!("门扇宽-{}", m.glass_cut);
    let g = vec![
        gr(s, "面-面玻", &gh, &gw, "1", "单双玻=双玻"),
        gr(s, "背-背玻", &gh, &gw, "1", "单双玻=双玻"),
        gr(s, "面玻", &gh, &gw, "1", "单双玻=单玻"),
        gr(s, "窗-窗固", "窗高-10", "上亮宽-10", "1", "窗高>0"),
        gr(s, "窗-窗背", "窗高-10", "上亮宽-10", "1", "窗高>0"),
    ];
    (p, g)
}
// ============================================================================
// 推拉门型号参数表(按用户提供的分类表顺序)
// rail 数值=光企高相对「含轨道的高度」的扣减量;轨道类型统一 地轨/吊轨/自粘下轨
// src: A=截图确认  B=读图日志(数值可靠,名称按行业规范归一)  C=同系列推定
// ============================================================================
static SLIDES: &[Slide] = &[
    // ---------- 推拉门 / 单轨推拉 ----------
    Slide { series: "单轨推拉模板", base: "总高", src: "B",
        rail: &[("地轨", 80), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=1", "总宽"), ("扇数=2", "总宽/2"), ("扇数=3", "总宽/3"), ("扇数=4", "总宽/4")],
        glass_h: 22, glass_w: "上下方算法变量-21-28", win_h: "上亮高-60+5",
        edge: "总高", track: "总宽-20-1020", ..EMPTY_SLIDE },
    Slide { series: "1645-单轨推拉门", src: "A",
        rail: &[("地轨", 75), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+16+16)/2+28"), ("扇数=3", "(总宽-26+16+16)/3+28"),
               ("扇数=4", "(总宽-26+16+16)/4+15+2")],
        glass_h: 23, glass_w: "上下方算法变量-23", win_h: "上亮高-60+5",
        track: "总宽-40+1020", case_h: "总高-28+36-510", case_w: "总宽-40+71",
        ..EMPTY_SLIDE },
    Slide { series: "1635单轨推拉门", src: "A",
        rail: &[("地轨", 80), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=1", "总宽"), ("扇数=2", "总宽/2"), ("扇数=3", "总宽/3"), ("扇数=4", "总宽/4")],
        glass_h: 22, glass_w: "上下方算法变量-21-28", win_h: "上亮高-60+5",
        edge: "总高-1020", track: "总宽-20-1020", ..EMPTY_SLIDE },
    Slide { series: "1645单轨推拉门", src: "C",
        rail: &[("地轨", 75), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+16+16)/2+28"), ("扇数=3", "(总宽-26+16+16)/3+28"),
               ("扇数=4", "(总宽-26+16+16)/4+15+2")],
        glass_h: 23, glass_w: "上下方算法变量-23", win_h: "上亮高-60+5",
        ..EMPTY_SLIDE },
    // ---------- 推拉门 / 两轨推拉 ----------
    Slide { series: "两轨推拉模板", src: "B",
        rail: &[("地轨", 80), ("吊轨", 85), ("自粘下轨", 90)],
        sxf: &[("扇数=2", "(总宽-26+16)/2+28"),
               ("扇数=3 and 重叠边数量=1", "(总宽-26+16)/3+28"),
               ("扇数=3 and 重叠边数量=2", "(总宽-26+16+16)/3+28"),
               ("扇数=4", "(总宽-26+16)/4+28")],
        glass_h: 22, glass_w: "上下方算法变量-50", win_h: "上亮高-60+5",
        ..EMPTY_SLIDE },
    Slide { series: "1635双玻推拉门", src: "A",
        rail: &[("地轨", 82), ("吊轨", 80), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+16)/2+28"),
               ("扇数=3 and 重叠边数量=1", "(总宽-26+16+16)/3+28"),
               ("扇数=3 and 重叠边数量=2", "(总宽-26+16+16)/3+28"),
               ("扇数=4", "(总宽-26+16+16-15)/4+2")],
        glass_h: 21, glass_w: "上下方算法变量-21-28", win_h: "上亮高-10",
        case_h: "总高-20+36-510", case_w: "总宽-40+71",
        win_w: &["(总宽-28-30)/2", "(总宽-28-30-30)/2", "(总宽-28-30-30)/3", "(总宽-28-30-30-30-30)/4"],
        ..EMPTY_SLIDE },
    Slide { series: "1645双玻推拉门", src: "B",
        rail: &[("地轨", 90), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+16)/2+28"), ("扇数=3", "(总宽-26+16+16)/3+28"),
               ("扇数=4", "(总宽-26+16+16-13)/4+2")],
        glass_h: 21, glass_w: "上下方算法变量-21-28", win_h: "上亮高-10",
        ..EMPTY_SLIDE },
    Slide { series: "1645卡枪灰推拉门", src: "A",
        rail: &[("地轨", 80), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+16)/2+28"),
               ("扇数=3 and 重叠边数量=1", "(总宽-26+16+16)/3+28"),
               ("扇数=3 and 重叠边数量=2", "(总宽-26+16+16)/3+28"),
               ("扇数=4", "(总宽-26+16+16+16)/4+4+2")],
        glass_h: 21, glass_w: "上下方算法变量-22-28", win_h: "上亮高-60+5",
        ..EMPTY_SLIDE },
    Slide { series: "1635推拉门吊轨鹏飞", src: "C",
        rail: &[("地轨", 82), ("吊轨", 80), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+16)/2+28"),
               ("扇数=3 and 重叠边数量=1", "(总宽-26+16+16)/3+28"),
               ("扇数=3 and 重叠边数量=2", "(总宽-26+16+16)/3+28"),
               ("扇数=4", "(总宽-26+16+16-15)/4+2")],
        glass_h: 21, glass_w: "上下方算法变量-21-28", win_h: "上亮高-10",
        ..EMPTY_SLIDE },
    Slide { series: "鹏飞1835月光灰推拉门", src: "B",
        rail: &[("地轨", 80), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+18)/2+28"),
               ("扇数=3 and 重叠边数量=1", "(总宽-26+18+18)/3+28"),
               ("扇数=3 and 重叠边数量=2", "(总宽-26+18+18)/3+28"),
               ("扇数=4", "(总宽-26+18+18)/4+18+4")],
        glass_h: 23, glass_w: "上下方算法变量-23-26", win_h: "上亮高-60+5",
        ..EMPTY_SLIDE },
    Slide { series: "1835推拉门", src: "A",
        rail: &[("地轨", 82), ("吊轨", 92), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+18)/2+28"),
               ("扇数=3 and 重叠边数量=1", "(总宽-26+18+18)/2+28"),
               ("扇数=3 and 重叠边数量=2", "(总宽-26+18+18)/3+28"),
               ("扇数=4", "(总宽-26+18+18+18)/4+2")],
        glass_h: 21, glass_w: "上下方算法变量-21-28", win_h: "上亮高-10",
        win_w: &["(总宽-28-30)/2", "(总宽-28-30-30)/2", "(总宽-28-30-30-30)/3", "(总宽-28-30-30-30-30)/4"],
        ..EMPTY_SLIDE },
    Slide { series: "鹏飞1835三连动推拉门", src: "A",
        rail: &[("地轨", 80), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+18)/2+28"),
               ("扇数=3 and 重叠边数量=1", "(总宽-26+18+18)/3+28"),
               ("扇数=3 and 重叠边数量=2", "(总宽-26+18+18)/3+28"),
               ("扇数=4", "(总宽-26+18+18)/4+15+2")],
        glass_h: 28, glass_w: "上下方算法变量-28-28", win_h: "上亮高-60+5",
        ..EMPTY_SLIDE },
    Slide { series: "1635四连动", src: "C",
        rail: &[("地轨", 82), ("吊轨", 80), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+16)/2+28"), ("扇数=3", "(总宽-26+16+16)/3+28"),
               ("扇数=4", "(总宽-26+16+16+16)/4+2")],
        glass_h: 21, glass_w: "上下方算法变量-21-28", win_h: "上亮高-10",
        ..EMPTY_SLIDE },
    Slide { series: "16*45三联动推拉-测试工时", src: "A",
        rail: &[("地轨", 80), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+16)/3+28"), ("扇数=3", "(总宽-26+16)/3+28"),
               ("扇数=4", "(总宽-26+16)/3+28")],
        glass_h: 23, glass_w: "上下方算法变量-23-28", win_h: "上亮高-60+5",
        edge: "含轨道的高度", track: "总宽-40", case_h: "总高-20+36-510", case_w: "总宽-40+71",
        win_w: &["(总宽-26-30)", "(总宽-26-30-30)/2", "(总宽-26-30-30-30)/3", "(总宽-26-30-30-30-30)/4"],
        ..EMPTY_SLIDE },
    // ---------- 推拉门 / 三轨推拉 ----------
    Slide { series: "测试的三轨推拉", src: "B",
        rail: &[("地轨", 90), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "总宽/2"), ("扇数=3", "总宽/3"), ("扇数=4", "总宽/4")],
        glass_h: 23, glass_w: "上下方算法变量-29-28", win_h: "上亮高-60+5",
        edge: "总高", track: "总宽-40", ..EMPTY_SLIDE },
    // ---------- 1645 分组 ----------
    Slide { series: "1635单玻三联动推拉门", src: "C",
        rail: &[("地轨", 75), ("吊轨", 85), ("自粘下轨", 90)],
        sxf: &[("扇数=2", "(总宽-26+16)/2+28"), ("扇数=3", "(总宽-26+16)/3-28"),
               ("扇数=4", "(总宽-26+16+16)/4-15+2")],
        glass_h: 23, glass_w: "上下方算法变量-22-28", win_h: "上亮高-60+5",
        single: true, ..EMPTY_SLIDE },
    // ---------- 7530推拉门 / 新增系列1 ----------
    Slide { series: "6033推拉门带三金刚网", src: "B",
        rail: &[("地轨", 62), ("吊轨", 55), ("自粘下轨", 50)],
        sxf: &[("扇数=2", "(总宽-45+60)/2"), ("扇数=3", "(总宽-45+60+60)/3"),
               ("扇数=4", "(总宽-45+60+60+60)/4")],
        glass_h: 104, glass_w: "上下方算法变量-104", win_h: "上亮高-60+5",
        track: "总宽-60-1020", ..EMPTY_SLIDE },
    Slide { series: "平安鑫6035推拉门", src: "A",
        rail: &[("地轨", 56), ("吊轨", 55), ("自粘下轨", 55)],
        sxf: &[("扇数=2", "(总宽-45+60)/2"),
               ("扇数=3 and 重叠边数量=1", "(总宽-45+60)/3"),
               ("扇数=3 and 重叠边数量=2", "(总宽-45+60+60)/3"),
               ("扇数=4", "(总宽-45+60+60)/4")],
        glass_h: 105, glass_w: "上下方算法变量-105", win_h: "上亮高-60+5",
        edge: "含轨道的高度", track: "总宽-60", case_h: "总高", case_w: "总宽",
        ..EMPTY_SLIDE },
    Slide { series: "平安鑫6035三推拉门带砂", src: "A",
        rail: &[("地轨", 56), ("吊轨", 55), ("自粘下轨", 55)],
        sxf: &[("扇数=2", "(总宽-45+60)/2"),
               ("扇数=3 and 重叠边数量=1", "(总宽-45+60)/3"),
               ("扇数=3 and 重叠边数量=2", "(总宽-45+60+60)/3"),
               ("扇数=4", "(总宽-45+60+60)/4")],
        glass_h: 105, glass_w: "上下方算法变量-105", win_h: "上亮高-60+5",
        edge: "含轨道的高度", track: "总宽-60", case_h: "总高", case_w: "总宽",
        ..EMPTY_SLIDE },
    Slide { series: "7535平安鑫推拉门", src: "A",
        rail: &[("地轨", 60), ("吊轨", 55), ("自粘下轨", 50)],
        sxf: &[("扇数=2", "(总宽-43+75)/2"),
               ("扇数=3 and 重叠边数量=1", "(总宽-43+75)/3"),
               ("扇数=3 and 重叠边数量=2", "(总宽-43+75+75)/3"),
               ("扇数=4", "(总宽-43+75+75)/4")],
        glass_h: 130, glass_w: "上下方算法变量-130", win_h: "上亮高-10",
        track: "总宽-60-1020", case_h: "总高", case_w: "总宽",
        win_w: &["(总宽-45-30)/2", "(总宽-45-30-30)/2", "(总宽-45-30-30-30)/3", "(总宽-45-30-30-30-30)/4"],
        ..EMPTY_SLIDE },
    Slide { series: "9040鹏飞推拉门", src: "A",
        rail: &[("地轨", 62), ("吊轨", 55), ("自粘下轨", 55)],
        sxf: &[("扇数=2", "(总宽-43+90)/2"),
               ("扇数=3 and 重叠边数量=1", "(总宽-43+90)/3"),
               ("扇数=3 and 重叠边数量=2", "(总宽-43+90+90)/3"),
               ("扇数=4", "(总宽-43+90+90)/4")],
        glass_h: 165, glass_w: "上下方算法变量-165", win_h: "上亮高-60+5",
        track: "总宽-60-1020", case_h: "总高", case_w: "总宽",
        ..EMPTY_SLIDE },
    Slide { series: "8533鹏飞推拉门", src: "B",
        rail: &[("地轨", 62), ("吊轨", 55), ("自粘下轨", 50)],
        sxf: &[("扇数=2", "(总宽-45+85)/2"),
               ("扇数=3 and 重叠边数量=1", "(总宽-45+85)/3"),
               ("扇数=3 and 重叠边数量=2", "(总宽-45+85+85)/3"),
               ("扇数=4", "(总宽-45+85+85)/4")],
        glass_h: 150, glass_w: "上下方算法变量-150", win_h: "上亮高-10",
        track: "总宽-60-1020", case_h: "总高-28+85-510", case_w: "总宽-60-169",
        ..EMPTY_SLIDE },
    Slide { series: "4533鹏飞三带纱推拉门", src: "C",
        rail: &[("地轨", 62), ("吊轨", 55), ("自粘下轨", 50)],
        sxf: &[("扇数=2", "(总宽-43+75)/2"),
               ("扇数=3 and 重叠边数量=1", "(总宽-43+75)/3"),
               ("扇数=3 and 重叠边数量=2", "(总宽-43+75+75)/3"),
               ("扇数=4", "(总宽-43+75+75)/4")],
        glass_h: 130, glass_w: "上下方算法变量-130", win_h: "上亮高-10",
        track: "总宽-60-1020", case_h: "总高", case_w: "总宽",
        ..EMPTY_SLIDE },
    // ---------- 1340 分组 ----------
    Slide { series: "1340三连动推拉门", src: "A",
        rail: &[("地轨", 111), ("吊轨", 90), ("自粘下轨", 75)],
        sxf: &[("扇数=2", "(总宽-26+13)/2+28"), ("扇数=3", "(总宽-26+13+13)/3+28"),
               ("扇数=4", "(总宽-26+13+13)/4-15+2")],
        glass_h: 17, glass_w: "上下方算法变量-18-28", win_h: "上亮高-60+5",
        track: "总宽-40-1020", case_h: "总高-20+36", case_w: "总宽-40+71",
        ..EMPTY_SLIDE },
];

// ============================================================================
// 平开门型号参数表(分类表可见 13 个 + 读图日志记录的 14 个,去重)
// src: B=读图日志  C=同系列推定(截图未覆盖/原软件为空)
// ============================================================================
static FLATS: &[Flat] = &[
    // ---- 读图日志已有明确公式 ----
    Flat { series: "双包平开模板", outer_h: "总高", door_h: "总高-30", door_w: "总宽-26", glass_cut: 22, src: "B" },
    Flat { series: "1635双包套平开门", outer_h: "总高-510", door_h: "总高-48", door_w: "总宽-72", glass_cut: 150, src: "B" },
    Flat { series: "7535双包套", outer_h: "总高-510", door_h: "总高-31", door_w: "总宽-42", glass_cut: 130, src: "B" },
    Flat { series: "1645双包套", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-42", glass_cut: 23, src: "B" },
    Flat { series: "1635双包套平开门小材料", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-40", glass_cut: 22, src: "B" },
    Flat { series: "单包平开模板", outer_h: "总高", door_h: "总高-30", door_w: "总宽-26", glass_cut: 22, src: "B" },
    Flat { series: "16*35单包平开", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-42", glass_cut: 21, src: "B" },
    Flat { series: "35*45平开窗单包平开", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-49", glass_cut: 76, src: "B" },
    Flat { series: "7535平开门", outer_h: "总高-610", door_h: "总高-40", door_w: "总宽-67", glass_cut: 130, src: "B" },
    Flat { series: "1635单包平开门小材料", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-40", glass_cut: 22, src: "B" },
    Flat { series: "16*45平开门", outer_h: "总高-510", door_h: "总高-31", door_w: "总宽-42", glass_cut: 21, src: "B" },
    Flat { series: "4535鹏飞单包平开门", outer_h: "总高-510", door_h: "总高-40", door_w: "总宽-42", glass_cut: 80, src: "B" },
    Flat { series: "鹏飞6083单包平开门", outer_h: "总高-510", door_h: "总高-90", door_w: "总宽-42", glass_cut: 102, src: "B" },
    // ---- 分类表可见,截图未覆盖公式,按同系列推定 ----
    Flat { series: "鹏飞6033单包平开门", outer_h: "总高-510", door_h: "总高-90", door_w: "总宽-42", glass_cut: 102, src: "C" },
    Flat { series: "7535门心", outer_h: "总高-510", door_h: "总高-40", door_w: "总宽-67", glass_cut: 130, src: "C" },
    Flat { series: "1635固定单玻", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-40", glass_cut: 22, src: "C" },
    Flat { series: "7535纱门", outer_h: "总高-510", door_h: "总高-40", door_w: "总宽-67", glass_cut: 130, src: "C" },
    Flat { series: "4030平开门", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-40", glass_cut: 70, src: "A" },
    Flat { series: "1635复古门", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-40", glass_cut: 22, src: "C" },
    Flat { series: "1645固定", outer_h: "总高-510", door_h: "总高-31", door_w: "总宽-42", glass_cut: 21, src: "C" },
    Flat { series: "7535带望窗平开门", outer_h: "总高-610", door_h: "总高-40", door_w: "总宽-67", glass_cut: 130, src: "C" },
    Flat { series: "1635卡枪灰双玻小材料", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-40", glass_cut: 22, src: "C" },
    Flat { series: "1645鹏飞铂金灰", outer_h: "总高-510", door_h: "总高-31", door_w: "总宽-42", glass_cut: 21, src: "C" },
    Flat { series: "1340单玻平开门", outer_h: "总高-510", door_h: "总高-30", door_w: "总宽-42", glass_cut: 17, src: "C" },
    Flat { series: "5830平开门", outer_h: "总高-510", door_h: "总高-40", door_w: "总宽-42", glass_cut: 80, src: "C" },
    Flat { series: "8535平开门", outer_h: "总高-510", door_h: "总高-90", door_w: "总宽-42", glass_cut: 102, src: "C" },
];

/// 统一历史 OCR 误读的轨道类型名称(让用户选「自粘下轨」时能命中公式)
pub fn normalize_rail_conditions(conn: &Connection) {
    let fixes: &[(&str, &str)] = &[
        ("轨道类型=自轨下轨", "轨道类型=自粘下轨"),
        ("轨道类型=自包下轨", "轨道类型=自粘下轨"),
        ("轨道类型=自格下轨", "轨道类型=自粘下轨"),
        ("轨道类型=百格下轨", "轨道类型=自粘下轨"),
        ("轨道类型=白格下轨", "轨道类型=自粘下轨"),
        ("轨道类型=百搭下轨", "轨道类型=自粘下轨"),
        ("轨道类型=自槽下滑", "轨道类型=自粘下轨"),
        ("轨道类型=自折吊轨", "轨道类型=自粘下轨"),
        ("轨道类型=自动下轨", "轨道类型=自粘下轨"),
        ("轨道类型=直接下轨", "轨道类型=自粘下轨"),
        ("轨道类型=双轨", "轨道类型=地轨"),
        ("轨道类型=三轨", "轨道类型=吊轨"),
        ("轨道类型=单轨", "轨道类型=地轨"),
    ];
    for (old, new) in fixes {
        conn.execute(
            "UPDATE profile_lib SET condition=REPLACE(condition,?1,?2) WHERE condition LIKE ?3",
            params![old, new, format!("%{}%", old)],
        )
        .ok();
        conn.execute(
            "UPDATE glass_lib SET condition=REPLACE(condition,?1,?2) WHERE condition LIKE ?3",
            params![old, new, format!("%{}%", old)],
        )
        .ok();
    }
}

/// 录屏修正·迁移段(必须在 seed 之前执行)
/// 把早期读图 OCR 误读的型号名改成录屏确认的写法,同时迁移旧库数据。
pub fn migrate_video_renames(conn: &Connection) {
    // 录屏 t=41 红字确认是「平安鑫」,早期读图误作「平安盛」
    for t in ["profile_lib", "glass_lib"] {
        conn.execute(
            &format!("UPDATE {} SET series=?1 WHERE series=?2", t),
            params!["35*45平安鑫单包平开", "35*45平安盛单包平开"],
        )
        .ok();
    }
}

/// 录屏修正·清理段(必须在 seed + dedup 之后执行,幂等)
/// 1) 空公式型号:清空所有公式行,只留一条 category='空公式' 占位,
///    型号仍出现在下拉里,但计算时不会产生任何型材/玻璃。
/// 2) 按录屏实测值纠正个别型号参数。
pub fn apply_video_corrections(conn: &Connection) {
    for &s in EMPTY_MODELS {
        conn.execute("DELETE FROM profile_lib WHERE series=?1", params![s]).ok();
        conn.execute("DELETE FROM glass_lib WHERE series=?1", params![s]).ok();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM profile_lib WHERE series=?1 AND category='空公式'",
                params![s],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            conn.execute(
                "INSERT INTO profile_lib \
                 (product_type,series,profile_name,formula,quantity,category,sort_order,condition,quantity_formula) \
                 VALUES('推拉门',?1,'（公式为空）','',0,'空公式',9999,'','')",
                params![s],
            )
            .ok();
        }
    }

    // 4030平开门:录屏 t=80 确认 门扇宽=总宽-40、玻璃扣减 70
    conn.execute(
        "UPDATE profile_lib SET formula='总宽-40' WHERE series='4030平开门' AND profile_name='门框宽'",
        [],
    )
    .ok();
    for col in ["height_formula", "width_formula"] {
        conn.execute(
            &format!(
                "UPDATE glass_lib SET {0}=REPLACE({0},'-22','-70') WHERE series='4030平开门'",
                col
            ),
            [],
        )
        .ok();
    }
}

/// 补全所有型号(幂等:按 5 元组去重,不覆盖用户自定义公式)
pub fn seed_all_models(conn: &Connection) {
    let mut profiles: Vec<PRow> = Vec::new();
    let mut glasses: Vec<GRow> = Vec::new();
    for m in SLIDES {
        let (p, g) = build_slide(m);
        profiles.extend(p);
        glasses.extend(g);
    }
    for m in FLATS {
        let (p, g) = build_flat(m);
        profiles.extend(p);
        glasses.extend(g);
    }

    // 空公式型号:只登记型号,不生成任何公式(录屏确认规则)
    profiles.retain(|r| !is_empty_model(&r.0));
    glasses.retain(|r| !is_empty_model(&r.0));

    // 已存在的 (product_type, series, name, condition, category) 集合
    let existing: HashSet<(String, String, String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT product_type, series, profile_name, condition, category FROM profile_lib")
            .expect("seed_all: select profile_lib");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, String>(2).unwrap_or_default(),
                    r.get::<_, String>(3).unwrap_or_default(),
                    r.get::<_, String>(4).unwrap_or_default(),
                ))
            })
            .expect("seed_all: query profile_lib");
        rows.filter_map(|x| x.ok()).collect()
    };
    let mut sort = existing.len() as i64;
    {
        let mut ins = conn
            .prepare(
                "INSERT INTO profile_lib(product_type, series, profile_name, formula, quantity, category, sort_order, condition, quantity_formula) \
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            )
            .expect("seed_all: prepare profile insert");
        for (series, name, formula, qty, cat, cond, qf) in &profiles {
            let pt = if FLATS.iter().any(|f| f.series == series) { "平开门" } else { "推拉门" };
            let key = (pt.to_string(), series.clone(), name.clone(), cond.clone(), cat.clone());
            if existing.contains(&key) {
                continue;
            }
            let qf = if qf.is_empty() {
                match name.as_str() {
                    "光企高" => "光企数量",
                    "勾企高" => "勾企数量",
                    "上下方" => "扇数*2",
                    _ => "",
                }
            } else { qf.as_str() };
            ins.execute(params![pt, series, name, formula, qty, cat, sort, cond, qf]).ok();
            sort += 1;
        }
    }

    let g_existing: HashSet<(String, String, String)> = {
        let mut stmt = conn
            .prepare("SELECT product_type, series, glass_name FROM glass_lib")
            .expect("seed_all: select glass_lib");
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0).unwrap_or_default(),
                    r.get::<_, String>(1).unwrap_or_default(),
                    r.get::<_, String>(2).unwrap_or_default(),
                ))
            })
            .expect("seed_all: query glass_lib");
        rows.filter_map(|x| x.ok()).collect()
    };
    let mut gsort = g_existing.len() as i64;
    {
        let mut ins = conn
            .prepare(
                "INSERT INTO glass_lib(product_type, series, glass_name, height_formula, width_formula, quantity_formula, condition, sort_order) \
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            )
            .expect("seed_all: prepare glass insert");
        for (series, name, hf, wf, qf, cond) in &glasses {
            let pt = if FLATS.iter().any(|f| f.series == series) { "平开门" } else { "推拉门" };
            let key = (pt.to_string(), series.clone(), name.clone());
            if g_existing.contains(&key) {
                continue;
            }
            ins.execute(params![pt, series, name, hf, wf, qf, cond, gsort]).ok();
            gsort += 1;
        }
    }
}

/// 供清单/导出使用:返回 (产品类型, 系列, 来源标记)
pub fn model_inventory() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut v = Vec::new();
    for m in SLIDES {
        v.push(("推拉门", m.series, m.src));
    }
    for m in FLATS {
        v.push(("平开门", m.series, m.src));
    }
    v
}
