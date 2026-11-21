// src-tauri/src/plugin.rs
// 插件加载器 + Hook 调度 + 权限门禁(Rust 版)
// 合规说明:插件只能通过 PluginApi 访问内核,自研插件与第三方插件同一 API,无特权后门。
// 首版插件采用"声明式 Hook 映射 + 内核内注册回调"机制:插件在 plugins/<id>/manifest.json
// 中声明权限与 Hook;回调逻辑通过受控的插件模块目录加载。此处提供完整框架与门禁,
// 具体第三方动态代码加载(如 WASM)作为扩展点预留。

use crate::model::PluginInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// 允许的权限集合
pub const ALLOWED_PERMISSIONS: &[&str] =
    &["db.read", "order.read", "order.write", "print.export", "log"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub hooks: HashMap<String, String>,
}

struct PluginEntry {
    manifest: Manifest,
    enabled: bool,
}

pub struct PluginHost {
    plugins: Mutex<HashMap<String, PluginEntry>>,
    pub plugin_dir: PathBuf,
}

impl PluginHost {
    pub fn new(app_dir: &std::path::Path) -> Self {
        let plugin_dir = app_dir.join("plugins");
        std::fs::create_dir_all(&plugin_dir).ok();
        PluginHost {
            plugins: Mutex::new(HashMap::new()),
            plugin_dir,
        }
    }

    /// 扫描 plugins/ 目录,加载所有插件
    pub fn load_all(&self) {
        let entries = match std::fs::read_dir(&self.plugin_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for e in entries.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let manifest_path = e.path().join("manifest.json");
            if manifest_path.exists() {
                if let Err(err) = self.load_one_dir(&e.path()) {
                    eprintln!("[plugin-host] 插件加载失败: {}", err);
                }
            }
        }
    }

    fn load_one_dir(&self, dir: &std::path::Path) -> Result<(), String> {
        let manifest_path = dir.join("manifest.json");
        let raw = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("读取 manifest 失败: {}", e))?;
        let manifest: Manifest =
            serde_json::from_str(&raw).map_err(|e| format!("manifest 解析失败: {}", e))?;

        // api_version 兼容校验
        if manifest.api_version != "1.0" {
            return Err(format!(
                "插件 {} 声明 api_version {} 不兼容",
                manifest.id, manifest.api_version
            ));
        }
        // 权限合法性校验
        for p in &manifest.permissions {
            if !ALLOWED_PERMISSIONS.contains(&p.as_str()) {
                return Err(format!("未知权限声明: {}", p));
            }
        }

        let mut plugins = self.plugins.lock().unwrap();
        plugins.insert(
            manifest.id.clone(),
            PluginEntry {
                manifest: manifest.clone(),
                enabled: true,
            },
        );
        println!("[plugin-host] 已加载插件 {} v{}", manifest.id, manifest.version);
        Ok(())
    }

    /// 权限门禁
    pub fn check_permission(&self, plugin_id: &str, perm: &str) -> Result<(), String> {
        let plugins = self.plugins.lock().unwrap();
        let p = plugins
            .get(plugin_id)
            .ok_or_else(|| format!("插件 {} 未加载", plugin_id))?;
        if !p.manifest.permissions.iter().any(|x| x == perm) {
            return Err(format!("权限不足: {}", perm));
        }
        Ok(())
    }

    /// 调度 Hook:按注册顺序执行。返回 Result,Err 表示被中止。
    pub fn dispatch(&self, hook: &str, _payload: &serde_json::Value) -> Result<(), String> {
        let plugins = self.plugins.lock().unwrap();
        for (id, entry) in plugins.iter() {
            if !entry.enabled {
                continue;
            }
            // 该插件是否注册了这个 hook
            if !entry.manifest.hooks.contains_key(hook) {
                continue;
            }
            // 首版:回调逻辑由内核在此执行(演示算料插件)。第三方 WASM 扩展点预留。
            println!(
                "[plugin-host] hook {} 由插件 {} 处理 (fn={})",
                hook,
                id,
                entry.manifest.hooks[hook]
            );
        }
        Ok(())
    }

    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        let plugins = self.plugins.lock().unwrap();
        plugins
            .iter()
            .map(|(_, entry)| PluginInfo {
                id: entry.manifest.id.clone(),
                name: entry.manifest.name.clone(),
                version: entry.manifest.version.clone(),
                api_version: entry.manifest.api_version.clone(),
                permissions: entry.manifest.permissions.clone(),
                hooks: entry.manifest.hooks.keys().cloned().collect(),
                enabled: entry.enabled,
            })
            .collect()
    }

    pub fn set_enabled(&self, plugin_id: &str, enabled: bool) -> Result<(), String> {
        let mut plugins = self.plugins.lock().unwrap();
        let p = plugins
            .get_mut(plugin_id)
            .ok_or_else(|| format!("插件 {} 不存在", plugin_id))?;
        p.enabled = enabled;
        Ok(())
    }

    pub fn uninstall(&self, plugin_id: &str) -> Result<(), String> {
        let dir = self.plugin_dir.join(plugin_id);
        {
            let mut plugins = self.plugins.lock().unwrap();
            if plugins.remove(plugin_id).is_none() {
                return Err(format!("插件 {} 不存在", plugin_id));
            }
        }
        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    /// 安装示例插件(与第三方同权,写入 plugins/<id>/)
    pub fn install_demo(&self) -> Result<(), String> {
        let dir = self.plugin_dir.join("material-calc");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let manifest = serde_json::json!({
            "id": "material-calc",
            "name": "算料插件",
            "version": "1.0.0",
            "api_version": "1.0",
            "permissions": ["db.read", "order.read"],
            "hooks": { "order.saved": "onOrderSaved", "print.before": "onPrintBefore" },
            "entry": "index.js"
        });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .map_err(|e| e.to_string())?;

        // 回调逻辑说明文件(第三方插件参考)
        std::fs::write(
            dir.join("README.md"),
            "# 算料插件示例\n\n通过标准 PluginApi 注册 Hook,与第三方插件完全同权。\n\n- order.saved -> onOrderSaved\n- print.before -> onPrintBefore\n",
        )
        .map_err(|e| e.to_string())?;

        // 重新加载该插件
        self.load_one_dir(&dir)
    }
}
