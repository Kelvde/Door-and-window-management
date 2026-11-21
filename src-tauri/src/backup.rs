// src-tauri/src/backup.rs
// 三套独立备份服务(关闭前 / 定时快照 / 手动):压缩 + 保留策略 + 删除
// 合规说明:仅做数据一致性备份,不含业务逻辑。默认压缩为 .sqlite.gz。

use crate::model::BackupRecord;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::{params, Connection};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const BACKUP_TYPES: [&str; 3] = ["shutdown", "snapshot", "manual"];

fn timestamp() -> String {
    use chrono::Local;
    // 带毫秒：同一秒内连点两次备份也不会撞名（VACUUM INTO 目标已存在会直接失败）
    Local::now().format("%Y%m%d-%H%M%S-%3f").to_string()
}

pub struct BackupService {
    pub db: Arc<Mutex<Connection>>,
}

impl BackupService {
    pub fn new(db: Arc<Mutex<Connection>>) -> Self {
        BackupService { db }
    }

    fn get_config(&self, key: &str) -> Option<String> {
        let conn = self.db.lock().unwrap();
        conn.query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .ok()
    }

    fn set_config(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.db.lock().unwrap();
        conn.execute(
            "INSERT INTO app_config(key,value) VALUES(?1,?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub fn cfg(&self, ty: &str) -> (PathBuf, usize, bool, String) {
        // 配置为空时回退到默认备份目录，避免 create_dir_all("") 直接失败
        let dir = self
            .get_config(&format!("backup.{}.dir", ty))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::db::backup_dir().join(ty));
        let retain = self
            .get_config(&format!("backup.{}.retain", ty))
            .and_then(|s| s.parse().ok())
            .unwrap_or(5usize);
        let compress = self.get_config("backup.compress").map(|s| s != "false").unwrap_or(true);
        let format = self.get_config("backup.format").unwrap_or_else(|| "gz".to_string());
        (dir, retain, compress, format)
    }

    /// 执行某类型备份。失败返回 Err,由调用方记录日志与提示,不阻塞主流程。
    pub fn run(&self, ty: &str) -> Result<BackupRecord, String> {
        if !BACKUP_TYPES.contains(&ty) {
            return Err(format!("未知备份类型: {}", ty));
        }
        let (dir, retain, compress, _format) = self.cfg(ty);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = timestamp();

        // 1) VACUUM INTO 生成一致性副本(不锁写、保证 WAL 一致)
        // 路径里的单引号必须转义，否则含 ' 的目录会让整条 SQL 语法错误。
        let tmp_name = format!(".tmp-{}.sqlite", stamp);
        let tmp_path = dir.join(&tmp_name);
        let tmp_path_str = tmp_path.to_string_lossy().replace('\\', "/").replace('\'', "''");
        {
            let conn = self.db.lock().unwrap();
            conn.execute(&format!("VACUUM INTO '{}'", tmp_path_str), [])
                .map_err(|e| e.to_string())?;
        }

        // 2) 压缩。只产出 gzip 内容，后缀必须是 .gz；
        //    以前 format=zip 时写的是 gzip 字节却叫 .zip，任何解压软件都打不开。
        let final_name;
        if compress {
            let data = std::fs::read(&tmp_path).map_err(|e| e.to_string())?;
            final_name = format!("{}-{}.sqlite.gz", ty, stamp);
            let mut enc = GzEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&data).map_err(|e| e.to_string())?;
            let compressed = enc.finish().map_err(|e| e.to_string())?;
            std::fs::write(dir.join(&final_name), compressed).map_err(|e| e.to_string())?;
            std::fs::remove_file(&tmp_path).ok();
        } else {
            final_name = format!("{}-{}.sqlite", ty, stamp);
            std::fs::rename(&tmp_path, dir.join(&final_name)).map_err(|e| e.to_string())?;
        }

        // 3) 记录到 backups 表
        let final_path = dir.join(&final_name);
        let size = std::fs::metadata(&final_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        {
            let conn = self.db.lock().unwrap();
            conn.execute(
                "INSERT INTO backups(backup_type, file_path, file_size, status) VALUES(?1,?2,?3,'ok')",
                params![ty, final_path.to_string_lossy().to_string(), size],
            )
            .map_err(|e| e.to_string())?;
        }

        // 4) 按保留策略滚动删除最旧。手动备份是全库备份，按用户要求无上限，不参与滚动删除。
        if ty != "manual" {
            self.rotate(ty, retain);
        }

        Ok(BackupRecord {
            id: 0,
            backup_type: ty.to_string(),
            file_path: final_path.to_string_lossy().to_string(),
            file_size: size,
            created_at: None,
            status: Some("ok".to_string()),
            name: final_name,
            exists: true,
        })
    }

    fn rotate(&self, ty: &str, retain: usize) {
        let (dir, _, _, _) = self.cfg(ty);
        let mut files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(&format!("{}-", ty)))
                        .unwrap_or(false)
                })
                .collect(),
            Err(_) => return,
        };
        // 按修改时间升序；读不到时间的排到最前（原来 None 也排最前，会误删刚生成的文件）
        files.sort_by_key(|p| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        });
        while files.len() > retain {
            if let Some(old) = files.first().cloned() {
                std::fs::remove_file(&old).ok();
                let conn = self.db.lock().unwrap();
                conn.execute(
                    "DELETE FROM backups WHERE file_path = ?1",
                    params![old.to_string_lossy().to_string()],
                )
                .ok();
            }
            files.remove(0);
        }
    }

    /// 列出某类备份。
    /// **以磁盘目录为准**：原来只查 backups 表，表里没登记的老备份永远看不到，
    /// 用户点了「恢复备份」就是一片空白。现在直接扫目录，表只作为补充信息。
    pub fn list(&self, ty: &str) -> Result<Vec<BackupRecord>, String> {
        // 「手动全库」这一类要把历史 snapshot / shutdown 目录里的整库备份也列出来，
        // 否则用户以前做的备份在恢复页里永远看不到（只显示 manual 目录，而它可能是空的）。
        let dirs: Vec<(String, PathBuf)> = if ty == "manual" {
            ["manual", "snapshot", "shutdown"]
                .iter()
                .map(|t| (t.to_string(), self.cfg(t).0))
                .collect()
        } else {
            vec![(ty.to_string(), self.cfg(ty).0)]
        };
        let mut out: Vec<BackupRecord> = Vec::new();

        for (src_ty, dir) in dirs {
            if !dir.is_dir() {
                continue;
            }
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if !p.is_file() {
                        continue;
                    }
                    let name = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default()
                        .to_string();
                    // 只认备份文件；跳过隐藏文件和 .trash 里的
                    if name.starts_with('.') {
                        continue;
                    }
                    let is_backup = name.ends_with(".sqlite.gz")
                        || name.ends_with(".sqlite")
                        || name.ends_with(".db");
                    if !is_backup {
                        continue;
                    }
                    let meta = entry.metadata().ok();
                    let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
                    let created = meta
                        .as_ref()
                        .and_then(|m| m.modified().ok())
                        .map(|t| {
                            chrono::DateTime::<chrono::Local>::from(t)
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string()
                        })
                        .unwrap_or_default();
                    out.push(BackupRecord {
                        id: 0,
                        backup_type: src_ty.clone(),
                        file_path: p.to_string_lossy().to_string(),
                        file_size: size,
                        created_at: Some(created),
                        status: Some("ok".to_string()),
                        name,
                        exists: true,
                    });
                }
            }
        }

        // 按时间倒序，最新的排最前
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    /// 删除单个备份:直接物理删除(不进任何回收区/回收站),调用方须先二次确认。
    /// 用户明确要求:删就是删,不要移到 .trash 之类的"回收区"里占地方。
    pub fn delete(&self, ty: &str, name: &str) -> Result<(), String> {
        Self::check_name(name).map_err(|e| e.to_string())?;
        let (dir, _, _, _) = self.cfg(ty);
        let src = dir.join(name);
        if !src.exists() {
            return Err("文件不存在".to_string());
        }
        std::fs::remove_file(&src).map_err(|e| e.to_string())?;
        let conn = self.db.lock().unwrap();
        conn.execute(
            "DELETE FROM backups WHERE backup_type = ?1 AND file_path = ?2",
            params![ty, src.to_string_lossy().to_string()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn clear(&self, ty: &str) -> Result<(), String> {
        let (dir, _, _, _) = self.cfg(ty);
        let files: Vec<String> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .filter(|n| n.starts_with(&format!("{}-", ty)))
                .collect(),
            Err(_) => return Ok(()),
        };
        for f in files {
            self.delete(ty, &f).ok();
        }
        Ok(())
    }

    /// 手动备份目录当前总字节数（点「立即备份」时用于 >2GB 提醒）
    pub fn manual_dir_size(&self) -> i64 {
        let (dir, _, _, _) = self.cfg("manual");
        let mut total = 0i64;
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if let Ok(m) = e.metadata() {
                    if m.is_file() {
                        total += m.len() as i64;
                    }
                }
            }
        }
        total
    }

    /// 准备恢复手动备份（整库）。运行时不能直接替换正在使用的 app.db，
    /// 这里先解压校验成 restore_pending.sqlite 并写标记，由下次启动时原子替换。
    /// 校验备份并准备恢复。
    /// 指定了类型就先去该类型目录找，找不到再退回手动备份目录
    /// （原来写死只找 manual，列表里显示其它目录的文件就会「备份文件不存在」）。
    pub fn prepare_restore(&self, ty: &str, name: &str) -> Result<String, String> {
        Self::check_name(name).map_err(|e| e.to_string())?;
        let mut candidates: Vec<PathBuf> = Vec::new();
        if !ty.is_empty() {
            candidates.push(self.cfg(ty).0.join(name));
        }
        candidates.push(self.cfg("manual").0.join(name));
        candidates.push(self.cfg("snapshot").0.join(name));
        candidates.push(self.cfg("shutdown").0.join(name));
        let src = candidates
            .into_iter()
            .find(|p| p.exists())
            .ok_or_else(|| "备份文件不存在".to_string())?;
        let data_dir = crate::db::data_dir();
        let pending = data_dir.join("restore_pending.sqlite");
        let raw = std::fs::read(&src).map_err(|e| e.to_string())?;
        let bytes: Vec<u8> = if name.ends_with(".gz") {
            use flate2::read::GzDecoder;
            use std::io::Read;
            let mut d = GzDecoder::new(&raw[..]);
            let mut out = Vec::new();
            d.read_to_end(&mut out).map_err(|e| e.to_string())?;
            out
        } else {
            raw
        };
        if !bytes.starts_with(b"SQLite format 3\0") {
            return Err("备份文件不是有效的 SQLite 数据库，已取消恢复".to_string());
        }
        std::fs::write(&pending, &bytes).map_err(|e| e.to_string())?;
        std::fs::write(data_dir.join("restore.flag"), name).map_err(|e| e.to_string())?;
        Ok("已校验备份并准备恢复，请关闭并重开软件后生效（恢复前会自动保留当前数据库）".to_string())
    }

    /// 备份文件名统一校验：只允许字母数字-_.
    /// 额外拒绝 ".."、以 "." 开头（.tmp/.trash 等内部文件）和空名，防路径穿越。
    fn check_name(name: &str) -> Result<(), &'static str> {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.starts_with('.')
            || name.contains("..")
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
        {
            return Err("非法文件名");
        }
        Ok(())
    }

    pub fn set_dir(&self, ty: &str, dir: &str) -> Result<(), String> {
        self.set_config(&format!("backup.{}.dir", ty), dir)
    }
    pub fn set_retain(&self, ty: &str, retain: &str) -> Result<(), String> {
        self.set_config(&format!("backup.{}.retain", ty), retain)
    }
}
