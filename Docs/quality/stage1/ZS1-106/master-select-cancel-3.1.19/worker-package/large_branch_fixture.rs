use zenpi::{core::{Turn,TurnRole},session::SessionStore};
fn main(){
 let path=std::path::PathBuf::from(std::env::var("ZS1_TUI_TREE_FIXTURE").unwrap());
 assert!(!path.exists());let cwd=std::path::PathBuf::from(std::env::var("ZS1_TUI_TREE_CWD").unwrap());
 let mut s=SessionStore::open_in_workspace(&path,&cwd).unwrap();s.enable_tree(Default::default(),&||false).unwrap();
 s.append_turn(Turn::new("fixture-common",TurnRole::User,"COMMON_A_FIXTURE")).unwrap();let common=s.active_tree_leaf().unwrap().to_string();
 for n in 0..1499 {s.append_turn(Turn::new(format!("fixture-b-{n}"),TurnRole::User,format!("BRANCH_B_ONLY_{n} {}","bounded-history ".repeat(3072)))).unwrap();}
 let target=s.active_tree_leaf().unwrap().to_string();s.select_tree_leaf(Some(&common),&||false).unwrap();
 println!("{}",serde_json::json!({"session_id":s.session_id(),"common":common,"target":target,"synthetic_entries":1500,"purpose":"fixture only; long abandoned B projection, no provider or tool execution"}));
}
