mod cm;
#[cfg(feature = "inline")]
mod inline;
#[cfg(target_os = "macos")]
mod macos;
mod remote;
use crate::common::SOFTWARE_UPDATE_URL;
use crate::ipc;
use hbb_common::{
    allow_err,
    config::{self, Config, Fav, PeerConfig, APP_NAME, ICON},
    log, sleep,
    tokio::{self, time},
};
use sciter::Value;
use std::{
    collections::HashMap,
    iter::FromIterator,
    process::Child,
    sync::{Arc, Mutex},
};
use virtual_display;

pub type Childs = Arc<Mutex<(bool, HashMap<(String, String), Child>)>>;

lazy_static::lazy_static! {
    // stupid workaround for https://sciter.com/forums/topic/crash-on-latest-tis-mac-sdk-sometimes/
    static ref STUPID_VALUES: Mutex<Vec<Arc<Vec<Value>>>> = Default::default();
}

#[derive(Default)]
struct UI(
    Childs,
    Arc<Mutex<(i32, bool)>>,
    Arc<Mutex<HashMap<String, String>>>,
);

struct UIHostHandler;

pub fn start(args: &mut [String]) {
    // https://github.com/c-smile/sciter-sdk/blob/master/include/sciter-x-types.h
    // https://github.com/rustdesk/rustdesk/issues/132#issuecomment-886069737

    use sciter::SCRIPT_RUNTIME_FEATURES::*;
    allow_err!(sciter::set_options(sciter::RuntimeOptions::ScriptFeatures(
        ALLOW_FILE_IO as u8 | ALLOW_SOCKET_IO as u8 | ALLOW_EVAL as u8 | ALLOW_SYSINFO as u8
    )));

    let mut frame = sciter::WindowBuilder::main_window().create();
    frame.set_title(APP_NAME);
    
    let page;
    if args.len() > 1 && args[0] == "--play" {
        args[0] = "--connect".to_owned();
        let path: std::path::PathBuf = (&args[1]).into();
        let id = path
            .file_stem()
            .map(|p| p.to_str().unwrap_or(""))
            .unwrap_or("")
            .to_owned();
        args[1] = id;
    }
    if args.is_empty() {
        let childs: Childs = Default::default();
        let cloned = childs.clone();
        std::thread::spawn(move || check_zombie(cloned));
        crate::common::check_software_update();
        frame.event_handler(UI::new(childs));
        frame.sciter_handler(UIHostHandler {});
        page = "index.html";
    } else if args[0] == "--install" {
        let childs: Childs = Default::default();
        frame.event_handler(UI::new(childs));
        frame.sciter_handler(UIHostHandler {});
        page = "install.html";
    } else if args[0] == "--cm" {
        frame.register_behavior("connection-manager", move || {
            Box::new(cm::ConnectionManager::new())
        });
        page = "cm.html";
    } else if (args[0] == "--connect"
        || args[0] == "--file-transfer"
        || args[0] == "--port-forward"
        || args[0] == "--rdp")
        && args.len() > 1
    {
        let mut iter = args.iter();
        let cmd = iter.next().unwrap().clone();
        let id = iter.next().unwrap().clone();
        let args: Vec<String> = iter.map(|x| x.clone()).collect();
        frame.set_title(&id);
        frame.register_behavior("native-remote", move || {
            Box::new(remote::Handler::new(cmd.clone(), id.clone(), args.clone()))
        });
        page = "remote.html";
    } else {
        log::error!("Wrong command: {:?}", args);
        return;
    }
    #[cfg(feature = "inline")]
    {
        let html = if page == "index.html" {
            inline::get_index()
        } else if page == "cm.html" {
            inline::get_cm()
        } else if page == "install.html" {
            inline::get_install()
        } else {
            inline::get_remote()
        };
        frame.load_html(html.as_bytes(), Some(page));
    }
    #[cfg(not(feature = "inline"))]
    frame.load_file(&format!(
        "file://{}/src/ui/{}",
        std::env::current_dir()
            .map(|c| c.display().to_string())
            .unwrap_or("".to_owned()),
        page
    ));
    frame.run_app();
}

impl UI {
    fn new(childs: Childs) -> Self {
        let res = check_connect_status(true);
        Self(childs, res.0, res.1)
    }

    fn recent_sessions_updated(&mut self) -> bool {
        let mut lock = self.0.lock().unwrap();
        if lock.0 {
            lock.0 = false;
            true
        } else {
            false
        }
    }

    fn get_id(&mut self) -> String {
        ipc::get_id()
    }

    fn get_password(&mut self) -> String {
        ipc::get_password()
    }

    fn update_password(&mut self, password: String) {
        if password.is_empty() {
            allow_err!(ipc::set_password(Config::get_auto_password()));
        } else {
            allow_err!(ipc::set_password(password));
        }
    }

    fn get_remote_id(&mut self) -> String {
        Config::get_remote_id()
    }

    fn set_remote_id(&mut self, id: String) {
        Config::set_remote_id(&id);
    }

    fn update_me(&self, _path: String) {
        // TODO: Redirect to release page specificed in build config (aur, apt:// scheme, etc.)
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("pkexec")
                .args(&["apt", "install", "-f", &_path])
                .spawn()
                .ok();
            std::fs::remove_file(&_path).ok();
            crate::run_me(Vec::<&str>::new()).ok();
        }
    }

    fn get_option(&self, key: String) -> String {
        if let Some(v) = self.2.lock().unwrap().get(&key) {
            v.to_owned()
        } else {
            "".to_owned()
        }
    }

    fn get_local_option(&self, key: String) -> String {
        Config::get_option(&key)
    }

    fn peer_has_password(&self, id: String) -> bool {
        !PeerConfig::load(&id).password.is_empty()
    }

    fn forget_password(&self, id: String) {
        let mut c = PeerConfig::load(&id);
        c.password.clear();
        c.store(&id);
    }

    fn get_peer_option(&self, id: String, name: String) -> String {
        let c = PeerConfig::load(&id);
        c.options.get(&name).unwrap_or(&"".to_owned()).to_owned()
    }

    fn set_peer_option(&self, id: String, name: String, value: String) {
        let mut c = PeerConfig::load(&id);
        if value.is_empty() {
            c.options.remove(&name);
        } else {
            c.options.insert(name, value);
        }
        c.store(&id);
    }

    fn get_options(&self) -> Value {
        let mut m = Value::map();
        for (k, v) in self.2.lock().unwrap().iter() {
            m.set_item(k, v);
        }
        m
    }

    fn test_if_valid_server(&self, host: String) -> String {
        hbb_common::socket_client::test_if_valid_server(&host)
    }

    fn get_sound_inputs(&self) -> Value {
        let mut a = Value::array(0);
        for name in get_sound_inputs() {
            a.push(name);
        a
    }

    fn set_options(&self, v: Value) {
        let mut m = HashMap::new();
        for (k, v) in v.items() {
            if let Some(k) = k.as_string() {
                if let Some(v) = v.as_string() {
                    if !v.is_empty() {
                        m.insert(k, v);
                    }
                }
            }
        }

        *self.2.lock().unwrap() = m.clone();
        ipc::set_options(m).ok();
    }

    fn set_option(&self, key: String, value: String) {
        let mut options = self.2.lock().unwrap();
        if value.is_empty() {
            options.remove(&key);
        } else {
            options.insert(key.clone(), value.clone());
        }
        ipc::set_options(options.clone()).ok();

        #[cfg(target_os = "macos")]
        if &key == "stop-service" {
            crate::platform::macos::launch(value != "Y");
        }
    }

    fn get_socks(&self) -> Value {
        let s = ipc::get_socks();
        match s {
            None => Value::null(),
            Some(s) => {
                let mut v = Value::array(0);
                v.push(s.proxy);
                v.push(s.username);
                v.push(s.password);
                v
            }
        }
    }

    fn set_socks(&self, proxy: String, username: String, password: String) {
        ipc::set_socks(config::Socks5Server {
            proxy,
            username,
            password,
        })
        .ok();
    }

    fn save_size(&mut self, x: i32, y: i32, w: i32, h: i32) {
        crate::server::input_service::fix_key_down_timeout_at_exit();
        Config::set_size(x, y, w, h);
    }

    fn get_size(&mut self) -> Value {
        let s = Config::get_size();
        let mut v = Value::array(0);
        v.push(s.0);
        v.push(s.1);
        v.push(s.2);
        v.push(s.3);
        v
    }

    fn get_connect_status(&mut self) -> Value {
        let mut v = Value::array(0);
        let x = *self.1.lock().unwrap();
        v.push(x.0);
        v.push(x.1);
        v
    }

    #[inline]
    fn get_peer_value(id: String, p: PeerConfig) -> Value {
        let values = vec![
            id,
            p.info.username.clone(),
            p.info.hostname.clone(),
            p.info.platform.clone(),
            p.options.get("alias").unwrap_or(&"".to_owned()).to_owned(),
        ];
        Value::from_iter(values)
    }

    fn get_peer(&self, id: String) -> Value {
        let c = PeerConfig::load(&id);
        Self::get_peer_value(id, c)
    }

    fn get_fav(&self) -> Value {
        Value::from_iter(Fav::load().peers)
    }

    fn store_fav(&self, fav: Value) {
        let mut tmp = vec![];
        fav.values().for_each(|v| {
            if let Some(v) = v.as_string() {
                if !v.is_empty() {
                    tmp.push(v);
                }
            }
        });
        Fav::store(tmp);
    }

    fn get_recent_sessions(&mut self) -> Value {
        let peers: Vec<Value> = PeerConfig::peers()
            .drain(..)
            .map(|p| Self::get_peer_value(p.0, p.2))
            .collect();
        Value::from_iter(peers)
    }

    fn get_icon(&mut self) -> String {
        ICON.to_owned()
    }

    fn remove_peer(&mut self, id: String) {
        PeerConfig::remove(&id);
    }

    fn new_remote(&mut self, id: String, remote_type: String) {
        let mut lock = self.0.lock().unwrap();
        let args = vec![format!("--{}", remote_type), id.clone()];
        let key = (id.clone(), remote_type.clone());
        if let Some(c) = lock.1.get_mut(&key) {
            if let Ok(Some(_)) = c.try_wait() {
                lock.1.remove(&key);
            } else {
                if remote_type == "rdp" {
                    allow_err!(c.kill());
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    c.try_wait().ok();
                    lock.1.remove(&key);
                } else {
                    return;
                }
            }
        }
        match crate::run_me(args) {
            Ok(child) => {
                lock.1.insert(key, child);
            }
            Err(err) => {
                log::error!("Failed to spawn remote: {}", err);
            }
        }
    }

    fn get_error(&mut self) -> String {
        let dtype = crate::platform::linux::get_display_server();
        if "wayland" == dtype {
            return "".to_owned();
        }
        if dtype != "x11" {
            return format!("Unsupported display server type {}, x11 expected!", dtype);
        }
    }

    fn is_login_wayland(&mut self) -> bool {
        return crate::platform::linux::is_login_wayland();
    }

    fn current_is_wayland(&mut self) -> bool {
        return crate::platform::linux::current_is_wayland();
    }

    fn get_software_update_url(&self) -> String {
        SOFTWARE_UPDATE_URL.lock().unwrap().clone()
    }

    fn get_new_version(&self) -> String {
        hbb_common::get_version_from_url(&*SOFTWARE_UPDATE_URL.lock().unwrap())
    }

    fn get_version(&self) -> String {
        crate::VERSION.to_owned()
    }

    fn get_app_name(&self) -> String {
        APP_NAME.to_owned()
    }

    fn discover(&self) {
        std::thread::spawn(move || {
            allow_err!(crate::rendezvous_mediator::discover());
        });
    }

    fn get_lan_peers(&self) -> String {
        config::LanPeers::load().peers
    }

    fn open_url(&self, url: String) {
        #[cfg(target_os = "linux")]
        let p = "xdg-open";
        allow_err!(std::process::Command::new(p).arg(url).spawn());
    }

    fn t(&self, name: String) -> String {
        crate::client::translate(name)
    }

    fn is_xfce(&self) -> bool {
        crate::platform::is_xfce()
    }
}

impl sciter::EventHandler for UI { // TODO: check unneed functions
    sciter::dispatch_script_call! {
        fn t(String);
        fn is_xfce();
        fn get_id();
        fn get_password();
        fn update_password(String);
        fn get_remote_id();
        fn set_remote_id(String);
        fn save_size(i32, i32, i32, i32);
        fn get_size();
        fn new_remote(String, bool);
        fn remove_peer(String);
        fn get_connect_status();
        fn get_recent_sessions();
        fn get_peer(String);
        fn get_fav();
        fn store_fav(Value);
        fn recent_sessions_updated();
        fn get_icon();
        fn install_me(String);
        fn is_installed();
        fn set_socks(String, String, String);
        fn get_socks();
        fn is_installed_lower_version();
        fn install_path();
        fn goto_install();
        fn is_process_trusted(bool);
        fn is_can_screen_recording(bool);
        fn is_installed_daemon(bool);
        fn get_error();
        fn is_login_wayland();
        fn fix_login_wayland();
        fn current_is_wayland();
        fn modify_default_login();
        fn get_options();
        fn get_option(String);
        fn get_local_option(String);
        fn get_peer_option(String, String);
        fn peer_has_password(String);
        fn forget_password(String);
        fn set_peer_option(String, String, String);
        fn test_if_valid_server(String);
        fn get_sound_inputs();
        fn set_options(Value);
        fn set_option(String, String);
        fn install_virtual_display();
        fn get_software_update_url();
        fn get_new_version();
        fn get_version();
        fn update_me(String);
        fn get_app_name();
        fn get_software_store_path();
        fn get_software_ext();
        fn open_url(String);
        fn create_shortcut(String);
        fn discover();
        fn get_lan_peers();
    }
}

impl sciter::host::HostHandler for UIHostHandler {
    fn on_graphics_critical_failure(&mut self) {
        log::error!("Critical rendering error: e.g. DirectX gfx driver error. Most probably bad gfx drivers.");
    }
}

pub fn check_zombie(childs: Childs) {
    let mut deads = Vec::new();
    loop {
        let mut lock = childs.lock().unwrap();
        let mut n = 0;
        for (id, c) in lock.1.iter_mut() {
            if let Ok(Some(_)) = c.try_wait() {
                deads.push(id.clone());
                n += 1;
            }
        }
        for ref id in deads.drain(..) {
            lock.1.remove(id);
        }
        if n > 0 {
            lock.0 = true;
        }
        drop(lock);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

// notice: avoiding create ipc connection repeatedly,
// because windows named pipe has serious memory leak issue.
#[tokio::main(flavor = "current_thread")]
async fn check_connect_status_(
    reconnect: bool,
    status: Arc<Mutex<(i32, bool)>>,
    options: Arc<Mutex<HashMap<String, String>>>,
) {
    let mut key_confirmed = false;
    loop {
        if let Ok(mut c) = ipc::connect(1000, "").await {
            let mut timer = time::interval(time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    res = c.next() => {
                        match res {
                            Err(err) => {
                                log::error!("ipc connection closed: {}", err);
                                break;
                            }
                            Ok(Some(ipc::Data::Options(Some(v)))) => {
                                *options.lock().unwrap() = v
                            }
                            Ok(Some(ipc::Data::OnlineStatus(Some((mut x, c))))) => {
                                if x > 0 {
                                    x = 1
                                }
                                key_confirmed = c;
                                *status.lock().unwrap() = (x as _, key_confirmed);
                            }
                            _ => {}
                        }
                    }
                    _ = timer.tick() => {
                        c.send(&ipc::Data::OnlineStatus(None)).await.ok();
                        c.send(&ipc::Data::Options(None)).await.ok();
                    }
                }
            }
        }
        if !reconnect {
            std::process::exit(0);
        }
        *status.lock().unwrap() = (-1, key_confirmed);
        sleep(1.).await;
    }
}

fn get_sound_inputs() -> Vec<String> {
    crate::platform::linux::get_pa_sources()
        .drain(..)
        .map(|x| x.1)
        .collect()
}

fn check_connect_status(
    reconnect: bool,
) -> (Arc<Mutex<(i32, bool)>>, Arc<Mutex<HashMap<String, String>>>) {
    let status = Arc::new(Mutex::new((0, false)));
    let options = Arc::new(Mutex::new(HashMap::new()));
    let cloned = status.clone();
    let cloned_options = options.clone();
    std::thread::spawn(move || check_connect_status_(reconnect, cloned, cloned_options));
    (status, options)
}

// sacrifice some memory
pub fn value_crash_workaround(values: &[Value]) -> Arc<Vec<Value>> {
    let persist = Arc::new(values.to_vec());
    STUPID_VALUES.lock().unwrap().push(persist.clone());
    persist
}
