use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use futures_util::stream::TryStreamExt;
use mongodb::{
    bson::{doc, oid::ObjectId, Document},
};
use serde::{Deserialize, Serialize};
use tauri::{command, State};

use crate::config::{run_bash_script, run_python_module, AppConfig};
use crate::mongo_manager::MongoClients;


#[derive(Debug, Deserialize)]
pub struct NewTask {
    pub name: String,
    pub template: String,
    pub templatefile: String,
    pub status: Option<String>,
    pub is_remote: Option<bool>, // 👈 新增字段，默认为 false
    pub task_type: Option<String>, // 👈 新增字段

}

#[derive(Debug, Serialize)]
pub struct CreatedTask {
    pub _id: String,
    pub name: String,
    pub template: String,
    pub templatefile: String,
    pub status: String,
    pub is_remote: Option<bool>, // 👈 新增字段，默认为 false
    pub task_type: Option<String>, // 👈 新增字段
}


#[command]
pub async fn create_task(new_task: NewTask,clients: State<'_, MongoClients>) -> Result<CreatedTask, String> {

    let client = &clients.local;

    let db = client.database("simulation_mission");
    let coll = db.collection::<Document>("tasks");

    let status = new_task.status.unwrap_or_else(|| "waiting".to_string());
    let is_remote = new_task.is_remote.unwrap_or(false);
    let task_type = new_task.task_type.clone().unwrap_or_else(|| "regular".to_string());

    let task_doc = doc! {
        "name": &new_task.name,
        "template": &new_task.template,
        "templatefile": &new_task.templatefile,
        "status": &status,
        "isRemote": Some(is_remote),
        "taskType": &task_type,
    };

    let result = coll.insert_one(task_doc, None)
        .await
        .map_err(|e| e.to_string())?;

    let inserted_id = result
        .inserted_id
        .as_object_id()
        .ok_or_else(|| "插入失败，无法获取ObjectId".to_string())?;

    Ok(CreatedTask {
        _id: inserted_id.to_hex(),
        name: new_task.name,
        template: new_task.template,
        templatefile: new_task.templatefile,
        status,
        is_remote: Some(is_remote),
        task_type: Some(task_type),
        
    })
}


#[command]
pub async fn generate_list(id: String,clients: State<'_, MongoClients>,
    config: State<'_, AppConfig>,) -> Result<(), String> {

    let client = &clients.local;
    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");
    let obj_id = ObjectId::parse_str(&id).map_err(|e| e.to_string())?;

    let task_doc = tasks
        .find_one(doc! { "_id": obj_id }, None)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务未找到".to_string())?;

    // 从任务中读取 template 和 name 字段
    let template = task_doc.get_str("templatefile").map_err(|_| "任务中缺少 template 字段".to_string())?;
    let name = task_doc.get_str("name").map_err(|_| "任务中缺少 name 字段".to_string())?;
    let task_type = task_doc.get_str("taskType").unwrap_or("regular"); // 默认为 regular
    let is_super = task_type.eq_ignore_ascii_case("super");
    // 执行python脚本
    let mut replacements = HashMap::new();
    replacements.insert("{template}", template);
    replacements.insert("{name}", name);
    replacements.insert("{extra_args}", if is_super { "--alpha_type super" } else { "" });

    run_python_module(&config, "generate_list", &replacements)?;
    // 可选：更新状态
    tasks
        .update_one(
            doc! { "_id": obj_id, "status": { "$ne": "deactive" } },
            doc! { "$set": { "status": "ready" } },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;


    Ok(())
}

#[derive(Deserialize)]
pub struct Updates {
    is_remote: bool,
}

#[tauri::command]
pub async fn update_task(id: String, updates: Updates,clients: State<'_, MongoClients>) -> Result<(), String> {
    let client = &clients.local;
    let db = client.database("simulation_mission");
    let collection = db.collection::<mongodb::bson::Document>("tasks");

    // 转换 id
    let obj_id = ObjectId::parse_str(&id)
        .map_err(|e| format!("解析 ObjectId 失败: {}", e))?;

    // 更新任务
    collection
        .update_one(
            doc! { "_id": obj_id },
            doc! { "$set": { "isRemote": updates.is_remote } },
            None,
        )
        .await
        .map_err(|e| format!("更新失败: {}", e))?;

    Ok(())
}


#[command]
pub fn read_latest_py_file(folder_path: String) -> Result<String, String> {
    let path = PathBuf::from(folder_path);
    if !path.is_dir() {
        return Err("路径不是文件夹".into());
    }

    let mut latest_file: Option<(SystemTime, PathBuf)> = None;

    for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        let modified = metadata.modified().map_err(|e| e.to_string())?;
        let file_path = entry.path();

        if file_path.extension().map(|ext| ext == "py").unwrap_or(false) {
            if let Some((latest_time, _)) = &latest_file {
                if modified > *latest_time {
                    latest_file = Some((modified, file_path));
                }
            } else {
                latest_file = Some((modified, file_path));
            }
        }
    }

    if let Some((_, path)) = latest_file {
        if let Some(file_name) = path.file_name() {
            Ok(file_name.to_string_lossy().to_string())
        } else {
            Err("无法获取文件名".into())
        }
    } else {
        Err("没有找到 .py 文件".into())
    }
}

#[command]
pub async fn sync_remote_task(alpha_mission_list: String, config: State<'_, AppConfig>) -> Result<(), String> {

    let (stdout, _) = run_bash_script("sync_remote_task", &[("alpha_mission_list", &alpha_mission_list)], false, &config).await?;
    println!("sync_remote_task 脚本输出:\n{}", stdout);
    Ok(())
}


#[derive(Serialize, Deserialize, Debug)]
pub struct Task {
    #[serde(rename = "_id")]
    pub id: ObjectId, // 用 id 显得更自然一些，但必须 rename "_id"
    pub name: String,
    pub template: String,
    pub templatefile: String,
    pub status: String,
    #[serde(rename = "isRemote")] // 👈 加上这个
    pub is_remote: Option<bool>, // 可选字段
    #[serde(rename = "taskType")] // 👈 加上这个
    pub task_type: Option<String>, // 或者 task_type: Option<String>

}

#[command]
pub async fn get_all_tasks(clients: State<'_, MongoClients>) -> Result<Vec<Task>, String> {
    let client = &clients.local;

    let db = client.database("simulation_mission");
    let collection = db.collection::<Document>("tasks");

    // 查找所有任务
    let mut cursor = collection.find(None, None).await.map_err(|e| e.to_string())?;

    // 用一个 Vec 来存储所有任务
    let mut tasks = Vec::new();

    // 遍历 cursor，获取每一个 Document
    while let Ok(Some(document)) = cursor.try_next().await {
        // 假设你有一个 `Task` 类型，使用 `from_document` 方法将 Document 转换为 Task
        if let Ok(task) = bson::from_document::<Task>(document) {
            tasks.push(task);
        }
    }

    Ok(tasks)
}

#[command]
pub async fn delete_task(id: String, clients: State<'_, MongoClients>) -> Result<(), String> {
    let client = &clients.local;

    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    let obj_id = ObjectId::parse_str(&id).map_err(|e| e.to_string())?;

    let update_result = tasks
        .update_one(
            doc! { "_id": obj_id },
            doc! { "$set": { "status": "deactive" } },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    if update_result.matched_count == 0 {
        return Err("未找到指定的任务".to_string());
    }


    // 找到该任务，获取 name 字段
    let task_doc = tasks
        .find_one(doc! { "_id": obj_id }, None)
        .await
        .map_err(|e| format!("查询任务失败: {}", e))?
        .ok_or_else(|| "未找到指定的任务".to_string())?;
    let task_name = task_doc
        .get_str("name")
        .map_err(|_| "任务中缺少 name 字段".to_string())?
        .to_string();
    // 删除 simulation_db 中的同名 collection（如果存在）
    let sim_data_db = client.database("simulation_db");
    if sim_data_db.list_collection_names(None).await
        .map_err(|e| e.to_string())?
        .contains(&task_name) 
    {
        sim_data_db
            .collection::<Document>(&task_name)
            .drop(None)
            .await
            .map_err(|e| format!("删除 simulation_db 中 collection 失败: {}", e))?;
    }
    // 如果是远程任务，尝试连接远程 Mongo 并删除同名 collection
    let is_remote = task_doc
        .get_bool("isRemote")
        .unwrap_or(false);
    if is_remote {
        let remote_client = &clients.remote;


        let remote_db = remote_client.database("simulation_db");

        if remote_db.list_collection_names(None).await
            .map_err(|e| e.to_string())?
            .contains(&task_name)
        {
            remote_db
                .collection::<Document>(&task_name)
                .drop(None)
                .await
                .map_err(|e| format!("删除远程 simulation_db 中 collection 失败: {}", e))?;
        }
    }

    Ok(())
}

#[command]
pub async fn start_task(task_name: String, config: String, is_remote: bool, clients: State<'_, MongoClients>, app_config: State<'_, AppConfig>) -> Result<(), String> {

    let (stdout, _) = run_bash_script(
        "start_task",
        &[("task_name", &task_name), ("config", &config)],
        is_remote,
        &app_config,
    )
    .await?;

    println!("start_task 脚本输出:\n{}", stdout);

    let client = &clients.local;

    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    tasks
        .update_one(
            doc! { "name": &task_name , "status": { "$ne": "deactive" }},
            doc! { "$set": { "status": "running" } },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[command]
pub async fn start_super_task(task_name: String, config: String, is_remote: bool, clients: State<'_, MongoClients>,app_config: State<'_, AppConfig>) -> Result<(), String> {
    let (stdout, _) = run_bash_script(
        "start_super_task",
        &[("task_name", &task_name), ("config", &config)],
        is_remote,
        &app_config,
    )
    .await?;

    println!("start_super_task 脚本输出:\n{}", stdout);

    let client = &clients.local;

    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    tasks
        .update_one(
            doc! { "name": &task_name , "status": { "$ne": "deactive" }},
            doc! { "$set": { "status": "running" } },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}


#[command]
pub async fn pause_task(task_name: String, is_remote: bool, clients: State<'_, MongoClients>, app_config: State<'_, AppConfig>) -> Result<(), String> {
    let (stdout, _) = run_bash_script(
        "pause_task",
        &[("task_name", &task_name)],
        is_remote,
        &app_config,
    )
    .await?;

    println!("pause_task 脚本输出:\n{}", stdout);
    let client = &clients.local;

    let db = client.database("simulation_mission");
    let tasks = db.collection::<Document>("tasks");

    tasks
        .update_one(
            doc! { "name": &task_name , "status": { "$ne": "deactive" }},
            doc! { "$set": { "status": "paused" } },
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}


