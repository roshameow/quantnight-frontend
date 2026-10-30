//! Finite test-process adapter. Every invocation executes actual CLI dispatch/core;
//! only TaskStore and original external-process I/O are synthetic. No canned DTO.
use super::*;
#[path = "../../../task-core/tests/support/original_professional_runtime.rs"]
mod original_professional_runtime;

#[tokio::test(flavor = "current_thread")]
#[ignore = "invoked by original Python watcher regression at subprocess boundary"]
async fn status_process() {
    let path = std::path::PathBuf::from(std::env::var_os("QUANTNIGHT_WATCH_IO").unwrap());
    let input: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let f = Fixture::new().await;
    // Reuse the original checker fixture, not a fabricated inspection DTO.
    let professional = input["originalChecker"].as_bool().unwrap_or(false)
        .then(|| original_professional_runtime::original_professional_fixture(false));
    if let Some(original) = &professional {
        let script=std::path::Path::new(&original.config.env.working_dir).join("scripts/check_tmux_python.sh");
        fs::write(f.root.join("scripts/check_tmux_python.sh"),format!("cd '{}'\nexec /bin/bash '{}' \"$@\"\n",original.config.env.working_dir,script.display())).unwrap();
    }
    let docs = |key: &str| -> Vec<Document> {
        input[key].as_array().unwrap().iter().cloned().map(|v| match Bson::try_from(v).unwrap() {
            Bson::Document(d) => d, _ => panic!("fixture document required")
        }).collect()
    };
    let name = input["name"].as_str().unwrap();
    {
        let mut data = f.store.data.lock().unwrap();
        data.fail = input.get("storeFailure").and_then(Value::as_str).map(str::to_owned);
        data.tasks.insert(Host::Local, docs("localControl"));
        data.tasks.insert(Host::Remote, docs("remoteControl"));
        data.collections.insert((Host::Local, name.into()), docs("localItems"));
        data.collections.insert((Host::Remote, name.into()), docs("remoteItems"));
        for (key, target) in [("afterSummary", &mut data.after_summary)] {
            if !input[key].is_null() {
                let host = if input[key]["remote"].as_bool().unwrap() { Host::Remote } else { Host::Local };
                let Bson::Document(row) = Bson::try_from(input[key]["task"].clone()).unwrap() else { panic!() };
                *target = Some((host, row));
            }
        }
        if !input["beforeUpdate"].is_null() {
            let v=&input["beforeUpdate"];
            let host=Host::from_remote(v["remote"].as_bool().unwrap());
            let Bson::Document(row)=Bson::try_from(v["task"].clone()).unwrap() else {panic!()};
            data.before_update=Some((host,row));
        }
    }
    if input.get("faults").is_some() { f.faults(input["faults"].clone()); }
    let args: Vec<&str> = input["argv"].as_array().unwrap().iter().map(|v|v.as_str().unwrap()).collect();
    let out = f.run(&args).await;
    let data = f.store.data.lock().unwrap();
    let rows = |h| data.tasks.get(&h).cloned().unwrap_or_default().into_iter()
        .map(|d| Bson::Document(d).into_relaxed_extjson()).collect::<Vec<_>>();
    let writes = data.writes.iter().map(|(h,q,u)| json!({"remote":h.is_remote(),
        "query":Bson::Document(q.clone()).into_relaxed_extjson(),
        "update":Bson::Document(u.clone()).into_relaxed_extjson()})).collect::<Vec<_>>();
    let queries = data.read_queries.iter().map(|(h,q)|json!({"remote":h.is_remote(),
        "query":Bson::Document(q.clone()).into_relaxed_extjson()})).collect::<Vec<_>>();
    fs::write(path.with_extension("out.json"), serde_json::to_vec(&json!({
        "outcome":out.value,"exitCode":out.exit_code,"localControl":rows(Host::Local),
        "remoteControl":rows(Host::Remote),"calls":data.calls,"writes":writes,"queries":queries,
        "wire":f.wire(),"checkerCalls":professional.as_ref().map(|original|
            fs::read_to_string(std::path::Path::new(&original.config.env.working_dir).join("python.calls")).unwrap_or_default()
                .lines().map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default()})).unwrap()).unwrap();
}

#[test]
fn original_python_observer_core_route() {
    let backend=std::path::PathBuf::from(std::env::var_os("QUANTNIGHT_BACKEND_SOURCE").unwrap());
    let output=std::process::Command::new(std::env::var_os("QUANTNIGHT_TEST_PYTHON").unwrap())
        .args(["-B","-m","pytest","-q","-p","no:cacheprovider","tests/test_mission_native_observation.py"])
        .current_dir(&backend)
        .env("QUANTNIGHT_WATCH_TEST_EXE",std::env::current_exe().unwrap())
        .env("PYTEST_DISABLE_PLUGIN_AUTOLOAD","1")
        .env("PYTEST_ADDOPTS", "")
        .output().unwrap();
    assert!(output.status.success(),"{}\n{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
    eprintln!("{}",String::from_utf8_lossy(&output.stdout));
}
