// src-tauri/src/media.rs
// 把单据导出成图片(PNG) / PPT：图片用系统自带 Edge 无头截图，PPT 是每页一张图片的最小 OpenXML 包。
// 不依赖 Office、不联网、不引入第三方 zip/图片库。

use std::path::{Path, PathBuf};

/// 找系统自带 Edge（Windows 10/11 均有）
pub fn find_edge() -> Option<PathBuf> {
    for p in [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ] {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    None
}

/// 本地路径转 file:/// URL（中文/空格做百分号编码，盘符冒号保留）
pub fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// 用 Edge 无头模式把一份 HTML 截成一张 PNG
/// 截图专用配置目录：**复用同一个**，Edge 不用每张都重新初始化 profile
/// （新 profile 首次启动要写几十个文件，单张要多花 0.5–1 秒）。
pub fn shot_profile(slot: usize) -> PathBuf {
    std::env::temp_dir().join(format!("dw_shot_profile_{}", slot))
}

/// 预热截图用的 Edge 配置目录。
/// 第一次用某个配置目录时，Edge 要初始化几十个文件（约 5–6 秒）；
/// 软件启动后空闲时先跑一遍，用户第一次导出就不用等这一下。
pub fn warm_up_profiles(count: usize) {
    let Some(edge) = find_edge() else { return };
    let blank = std::env::temp_dir().join("dw_warmup.html");
    if std::fs::write(&blank, "<html><head><meta charset=\"utf-8\"></head><body>warm</body></html>").is_err() {
        return;
    }
    std::thread::scope(|scope| {
        for slot in 0..count {
            let edge = &edge;
            let blank = &blank;
            scope.spawn(move || {
                let png = std::env::temp_dir().join(format!("dw_warmup_{}.png", slot));
                let _ = html_to_png(edge, blank, &png, 0, slot);
                let _ = std::fs::remove_file(&png);
            });
        }
    });
    let _ = std::fs::remove_file(&blank);
}

/// 用 Edge 无头模式把本地 HTML 截成 PNG。
/// 关键：**带超时**。原来用 Command::output() 无期限等待，Edge 一旦卡住
/// （profile 被占用、渲染进程挂掉），整个导出会永远不结束，界面看着就像死机。
pub fn html_to_png(
    edge: &Path,
    html_path: &Path,
    png_path: &Path,
    index: usize,
    slot: usize,
) -> Result<(), String> {
    let profile = shot_profile(slot);
    let _ = std::fs::create_dir_all(&profile);
    let mut child = std::process::Command::new(edge)
        .args([
            "--headless",
            "--disable-gpu",
            "--hide-scrollbars",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-background-networking",
            "--disable-sync",
            "--force-device-scale-factor=1",
            &format!("--user-data-dir={}", profile.to_string_lossy()),
            &format!("--screenshot={}", png_path.to_string_lossy()),
            "--window-size=900,1600",
            &file_url(html_path),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("启动截图进程失败：{}", e))?;

    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(45);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(png_path);
                    return Err(format!(
                        "第 {} 张截图超过 45 秒没完成（Edge 可能卡住），已中止。可再点一次导出重试",
                        index
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(format!("等待截图进程失败：{}", e)),
        }
    }

    if !png_path.exists() || std::fs::metadata(png_path).map(|m| m.len()).unwrap_or(0) < 1024 {
        return Err(format!("第 {} 张截图没有生成有效图片", index));
    }
    Ok(())
}

// ---------- 最小 ZIP（store 模式，无压缩）----------
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn zip_store(files: &[(String, Vec<u8>)], out: &Path) -> Result<(), String> {
    let mut body: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in files {
        let offset = body.len() as u32;
        let crc = crc32(data);
        let n = name.as_bytes();
        let len = data.len() as u32;
        // local file header
        body.extend_from_slice(&0x04034b50u32.to_le_bytes());
        body.extend_from_slice(&20u16.to_le_bytes()); // version needed
        body.extend_from_slice(&0u16.to_le_bytes()); // flags
        body.extend_from_slice(&0u16.to_le_bytes()); // method = store
        body.extend_from_slice(&0u16.to_le_bytes()); // time
        body.extend_from_slice(&0u16.to_le_bytes()); // date
        body.extend_from_slice(&crc.to_le_bytes());
        body.extend_from_slice(&len.to_le_bytes());
        body.extend_from_slice(&len.to_le_bytes());
        body.extend_from_slice(&(n.len() as u16).to_le_bytes());
        body.extend_from_slice(&0u16.to_le_bytes()); // extra len
        body.extend_from_slice(n);
        body.extend_from_slice(data);
        // central directory header
        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&0u16.to_le_bytes()); // method
        central.extend_from_slice(&0u16.to_le_bytes()); // time
        central.extend_from_slice(&0u16.to_le_bytes()); // date
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&(n.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk start
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(n);
    }
    let cd_offset = body.len() as u32;
    let cd_size = central.len() as u32;
    let count = files.len() as u16;
    body.extend_from_slice(&central);
    // EOCD
    body.extend_from_slice(&0x06054b50u32.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&cd_size.to_le_bytes());
    body.extend_from_slice(&cd_offset.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    std::fs::write(out, body).map_err(|e| e.to_string())
}

/// 从 PNG 头读出宽高，用于 PPT 里等比缩放（避免竖版单据被拉扁）
fn png_size(data: &[u8]) -> (u32, u32) {
    if data.len() >= 24 && &data[0..8] == b"\x89PNG\r\n\x1a\n" {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        if w > 0 && h > 0 {
            return (w, h);
        }
    }
    (900, 1600)
}

fn fit_picture(w: u32, h: u32) -> (u32, u32, u32, u32) {
    const SW: f64 = 9_144_000.0;
    const SH: f64 = 6_858_000.0;
    let scale = (SW / w.max(1) as f64).min(SH / h.max(1) as f64);
    let cx = (w as f64 * scale) as u32;
    let cy = (h as f64 * scale) as u32;
    let x = (SW as u32).saturating_sub(cx) / 2;
    let y = (SH as u32).saturating_sub(cy) / 2;
    (x, y, cx, cy)
}

/// 用一组 PNG 生成一个每页一张图的 PPTX
pub fn build_pptx(images: &[PathBuf], out: &Path) -> Result<(), String> {
    if images.is_empty() {
        return Err("没有可用于生成 PPT 的图片".to_string());
    }
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut ct = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\
<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>",
    );
    let mut sld_ids = String::new();
    let mut pres_rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for (i, img) in images.iter().enumerate() {
        let n = i + 1;
        ct.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>",
            n
        ));
        sld_ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{}\"/>", 255 + n, n));
        pres_rels.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{}.xml\"/>",
            n, n
        ));
        let bytes = std::fs::read(img).map_err(|e| e.to_string())?;
        let (iw, ih) = png_size(&bytes);
        let (px, py, pcx, pcy) = fit_picture(iw, ih);
        files.push((format!("ppt/media/image{}.png", n), bytes));
        let slide = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:cSld><p:spTree>\
<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>\
<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"单据图片\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>\
<p:blipFill><a:blip r:embed=\"rId1\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>\
<p:spPr><a:xfrm><a:off x=\"{px}\" y=\"{py}\"/><a:ext cx=\"{pcx}\" cy=\"{pcy}\"/></a:xfrm>\
<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>\
</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
            px = px, py = py, pcx = pcx, pcy = pcy
        );
        files.push((format!("ppt/slides/slide{}.xml", n), slide.into_bytes()));
        let rel = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/image{}.png\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>\
</Relationships>",
            n
        );
        files.push((format!("ppt/slides/_rels/slide{}.xml.rels", n), rel.into_bytes()));
    }
    ct.push_str("</Types>");
    pres_rels.push_str("<Relationship Id=\"rId999\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>");
    pres_rels.push_str("</Relationships>");
    let presentation = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId999\"/></p:sldMasterIdLst>\
<p:sldIdLst>{}</p:sldIdLst><p:sldSz cx=\"9144000\" cy=\"6858000\" type=\"screen4x3\"/>\
<p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>",
        sld_ids
    );
    files.push(("[Content_Types].xml".to_string(), ct.into_bytes()));
    files.push((
        "_rels/.rels".to_string(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/></Relationships>".as_bytes().to_vec(),
    ));
    files.push(("ppt/presentation.xml".to_string(), presentation.into_bytes()));
    files.push(("ppt/_rels/presentation.xml.rels".to_string(), pres_rels.into_bytes()));
    let master = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld>
<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
</p:sldMaster>"#;
    let layout = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1">
<p:cSld name="空白"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld>
<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#;
    let master_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#;
    let layout_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#;
    files.push(("ppt/slideMasters/slideMaster1.xml".to_string(), master.as_bytes().to_vec()));
    files.push(("ppt/slideMasters/_rels/slideMaster1.xml.rels".to_string(), master_rels.as_bytes().to_vec()));
    files.push(("ppt/slideLayouts/slideLayout1.xml".to_string(), layout.as_bytes().to_vec()));
    files.push(("ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_string(), layout_rels.as_bytes().to_vec()));
    zip_store(&files, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pptx_zip_structure() {
        let dir = std::env::temp_dir().join("dw_media_selftest");
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("page.png");
        // 带 IHDR 的伪 PNG 头，用来验证等比缩放取值
        let mut fake = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        fake.extend_from_slice(&[0u8; 4]); // chunk length
        fake.extend_from_slice(b"IHDR");   // chunk type（偏移 12..16）

        fake.extend_from_slice(&900u32.to_be_bytes());
        fake.extend_from_slice(&1600u32.to_be_bytes());
        fake.extend_from_slice(&[0u8; 16]);
        std::fs::write(&img, &fake).unwrap();
        assert_eq!(png_size(&fake), (900, 1600));
        assert!(fit_picture(900, 1600).2 > 0);

        let out = dir.join("selftest.pptx");
        build_pptx(&[img], &out).unwrap();
        let data = std::fs::read(&out).unwrap();
        assert!(data.starts_with(b"PK\x03\x04"), "缺 local file header");
        assert!(data.windows(4).any(|w| w == b"PK\x05\x06"), "缺 EOCD");
        assert!(data.windows(4).any(|w| w == b"PK\x01\x02"), "缺 central directory");
        assert!(String::from_utf8_lossy(&data).contains("ppt/presentation.xml"));
        // 每页图片保持宽高比，不能整页拉伸成 4:3
        let s = String::from_utf8_lossy(&data);
        assert!(s.contains("<a:off x=") && s.contains("cx=\""), "缺图片位置/尺寸");
    }
}

#[cfg(test)]
mod bench_shot {
    /// 手测截图耗时（需要本机有 Edge）：cargo test --release one_shot -- --ignored --nocapture
    #[test]
    #[ignore]
    fn one_shot() {
        let edge = super::find_edge().expect("没找到 Edge");
        let dir = std::env::temp_dir();
        let html = dir.join("dw_bench.html");
        std::fs::write(
            &html,
            "<html><head><meta charset=\"utf-8\"></head><body><h1>测试单据</h1><table border=1><tr><td>单号</td><td>TEST-1</td></tr><tr><td>尺寸</td><td>900×2100</td></tr></table></body></html>",
        )
        .unwrap();
        let mut total = 0u128;
        for i in 1..=5 {
            let png = dir.join(format!("dw_bench_{}.png", i));
            let t = std::time::Instant::now();
            super::html_to_png(&edge, &html, &png, i, 0).expect("截图失败");
            let ms = t.elapsed().as_millis();
            println!("第 {} 张: {} ms", i, ms);
            total += ms;
            let _ = std::fs::remove_file(&png);
        }
        println!("平均 {} ms/张（profile 复用后）", total / 5);
        let _ = std::fs::remove_file(&html);
    }

    /// 3 路并行 vs 串行的总耗时：cargo test --release three_way -- --ignored --nocapture
    #[test]
    #[ignore]
    fn three_way() {
        let edge = super::find_edge().expect("没找到 Edge");
        let dir = std::env::temp_dir();
        let html = dir.join("dw_bench3.html");
        std::fs::write(&html, "<html><head><meta charset=\"utf-8\"></head><body><h1>并发测试</h1><p>hello</p></body></html>").unwrap();
        let t = std::time::Instant::now();
        std::thread::scope(|scope| {
            for slot in 0..3usize {
                let edge = &edge;
                let html = &html;
                let dir = &dir;
                scope.spawn(move || {
                    for i in 0..4usize {
                        let png = dir.join(format!("dw_b_{}_{}.png", slot, i));
                        super::html_to_png(edge, html, &png, i, slot).expect("截图失败");
                        let _ = std::fs::remove_file(&png);
                    }
                });
            }
        });
        println!("3 路 × 4 张 = 12 张，总耗时 {} ms", t.elapsed().as_millis());
        let _ = std::fs::remove_file(&html);
    }
}
