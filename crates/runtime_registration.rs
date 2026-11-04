//! Non-secret registration schema, shared by native build/runtime consumers.
//! Does not select Mongo endpoints, launch services, derive paths, or parse TOML.
use serde::{Deserialize,Serialize};
use std::{fs,path::{Path,PathBuf}};
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationSelector { pub kind:String, pub path:PathBuf }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeEnrollment { pub executable:PathBuf, pub configuration:ConfigurationSelector, pub protocol_version:u32 }
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub schema_version:u32, pub frontend_root:PathBuf, pub backend_root:PathBuf,
    pub executable:PathBuf, pub target:String, pub profile:String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub runtime:Option<RuntimeEnrollment>,
}
impl Binding {
    pub fn read(path:&Path)->Result<Self,String>{
        if !path.is_absolute(){return Err("binding must be an explicit absolute path".into());}
        Self::from_bytes(&fs::read(path).map_err(|e|format!("binding unavailable: {e}"))?)
    }
    pub fn from_bytes(bytes:&[u8])->Result<Self,String>{
        let binding:Self=serde_json::from_slice(bytes).map_err(|_|"invalid binding schema")?;
        if !matches!((binding.schema_version,binding.runtime.is_some()),(1,false)|(2,true)) || binding.profile!="debug" || binding.target.is_empty(){return Err("unsupported binding schema/profile/target".into());}
        for root in [&binding.frontend_root,&binding.backend_root]{if !root.is_absolute() || !root.is_dir() || root.canonicalize().map_err(|e|e.to_string())?!=*root{return Err("binding roots must be explicit canonical directories".into());}}
        if binding.runtime.is_none() { check_executable_path(&binding.executable)?; }
        else if !binding.executable.is_absolute() || binding.executable.file_name().is_none() || binding.executable.components().any(|c|matches!(c,std::path::Component::CurDir|std::path::Component::ParentDir)) {
            return Err("original GUI selection must remain an absolute normalized path".into());
        }
        // Runtime clients retain the original GUI selection as metadata, not
        // a dependency on the GUI binary or its build output directory.
        if let Some(runtime)=&binding.runtime {
            check_executable_path(&runtime.executable)?;
            if runtime.executable!=binding.frontend_root.join("crates/target/debug/taskctl"){return Err("runtime executable must be the original independent crates/target/debug/taskctl; never the GUI host".into());}
            if runtime.protocol_version!=1{return Err("runtime requires protocol1".into());}
            match runtime.configuration.kind.as_str(){
                "backend_root" if runtime.configuration.path==binding.backend_root=>{},
                "app_config" if runtime.configuration.path.is_absolute() && runtime.configuration.path.file_name().is_some() && !runtime.configuration.path.components().any(|c|matches!(c,std::path::Component::CurDir|std::path::Component::ParentDir))=>{},
                _=>return Err("runtime configuration must be exact backend_root or an absolute normalized registered AppConfig metadata".into()),
            }
            if !binding.target.contains("apple-darwin"){return Err("runtime enrollment currently requires macOS; legacy standalone remains available".into());}
        }
        Ok(binding)
    }
}
pub fn check_executable_path(path:&Path)->Result<(),String>{
    if !path.is_absolute() || path.file_name().is_none() || path.components().any(|c|matches!(c,std::path::Component::CurDir|std::path::Component::ParentDir)){return Err("explicit canonical executable path required".into());}
    let parent=path.parent().ok_or("executable parent missing")?;
    if parent.canonicalize().map_err(|_|"executable parent unavailable")?!=parent{return Err("executable parent must be canonical".into());}Ok(())
}
