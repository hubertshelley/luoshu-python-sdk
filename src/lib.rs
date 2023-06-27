use luoshu::data::{
    ActionEnum, ConfigurationReg, Connection, LuoshuDataEnum, LuoshuDataHandle, LuoshuMemData,
    ServiceReg, Subscribe,
};
use luoshu_registry::Service;
use pyo3::{exceptions};
use pyo3::prelude::*;
use pyo3::types::{PyString};
use std::collections::HashMap;

use std::sync::{Arc, Mutex as std_Mutex};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::signal;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;

#[pyclass]
struct Luoshu {
    luoshu_host: String,
    luoshu_port: u16,
    namespace: String,
    name: String,
    host: String,
    port: u16,
    subscribe_book: Arc<std_Mutex<HashMap<String, UnboundedSender<ConfigurationReg>>>>,
    subscribe_sender: UnboundedSender<Subscribe>,
    subscribe_receiver: Arc<Mutex<UnboundedReceiver<Subscribe>>>,
    config_map: Arc<std_Mutex<HashMap<String, String>>>,
}

#[pymethods]
impl Luoshu {
    #[new]
    fn new(namespace: String, name: String, host: String, port: u16, luoshu_host: Option<String>, luoshu_port: Option<u16>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Subscribe>();
        let luoshu_host = luoshu_host.unwrap_or("127.0.0.1".to_string());
        let luoshu_port = luoshu_port.unwrap_or(19998);
        Self {
            luoshu_host,
            luoshu_port,
            namespace,
            name,
            host,
            port,
            subscribe_book: Arc::new(std_Mutex::new(HashMap::new())),
            subscribe_sender: tx,
            subscribe_receiver: Arc::new(Mutex::new(rx)),
            config_map: Arc::new(std_Mutex::new(HashMap::new())),
        }
    }

    pub fn config_subscribe<'p>(
        self_: PyRefMut<'p, Self>,
        py: Python<'p>,
        namespace: String,
        config_name: String,
    ) -> PyResult<&'p PyAny> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ConfigurationReg>();
        let subscribe = Subscribe::new(namespace, config_name);
        self_
            .subscribe_book
            .lock()
            .unwrap()
            .insert(subscribe.to_string(), tx);
        self_
            .subscribe_sender
            .send(subscribe)
            .expect("callback error");
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut result = "".to_string();
            #[cfg(unix)]
                let terminate = async {
                signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("failed to install signal handler")
                    .recv()
                    .await;
            };

            #[cfg(not(unix))]
                let terminate = async { let _ = std::future::pending::<()>(); };
            tokio::select! {
                Some(config) = rx.recv() =>{
                    result = serde_json::to_string(&config).expect("callback error")
                }
                // 监听程序被中断
                _ = signal::ctrl_c() => {
                }
                // 监听程序被中断
                _ = terminate => {
                }
            }
            let result: Py<PyString> = Python::with_gil(|py| PyString::new(
                py,
                result.as_str(),
            ).into_py(py));
            Ok(result)
        })
    }

    pub fn process<'p>(self_: PyRef<'p, Self>, py: Python<'p>) -> PyResult<&'p PyAny> {
        let luoshu_addr = format!("{}:{}", self_.luoshu_host, self_.luoshu_port);
        let namespace = self_.namespace.clone();
        let name = self_.name.clone();
        let host = self_.host.clone();
        let port = self_.port;
        let config_map = self_.config_map.clone();
        let subscribe_book = self_.subscribe_book.clone();
        let subscribe_receiver = self_.subscribe_receiver.clone();
        let callback = move |x: ConfigurationReg| {
            let subscribe_str = x.get_subscribe_str();
            let config_str = serde_json::to_string(&x.config).unwrap();
            if let Some(sender) = subscribe_book.lock().unwrap().get(subscribe_str.as_str()) {
                if config_map.lock().unwrap().get(subscribe_str.as_str()) == Some(&config_str) {
                    sender.send(x).expect("callback error");
                }
            } else {
                config_map.lock().unwrap().insert(subscribe_str, serde_json::to_string(&x.config).unwrap());
            }
        };
        pyo3_asyncio::tokio::future_into_py(py, async move {
            process(luoshu_addr, namespace, name, host, port, callback, subscribe_receiver)
                .await
                .map_err(|e| exceptions::PyException::new_err(e.to_string()))?;
            Ok(Python::with_gil(|py| py.None()))
        })
    }
}

async fn process<T: Fn(ConfigurationReg)>(
    luoshu_addr: String,
    namespace: String,
    name: String,
    host: String,
    port: u16,
    callback: T,
    subscribe_receiver: Arc<Mutex<UnboundedReceiver<Subscribe>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = Arc::new(Mutex::new(LuoshuMemData::new()));
    let addr = luoshu_addr;
    let stream = TcpStream::connect(addr.clone()).await.unwrap();
    let mut connection = Connection::new(stream, addr.parse()?);
    // 生成服务数据
    let frame =
        ActionEnum::Up(ServiceReg::new(namespace, name, Service::new(host, port)).into()).into();
    connection.write_frame(&frame).await?;
    let time_sleep = || async {
        tokio::time::sleep(Duration::from_secs(5)).await;
        true
    };
    let mut subscribe_receiver = subscribe_receiver.lock().await;
    loop {
        #[cfg(unix)]
            let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
            let terminate = async { let _ = std::future::pending::<()>(); };
        tokio::select! {
            Some(subscribe) = subscribe_receiver.recv()=>{
                connection.write_frame(&ActionEnum::Subscribe(subscribe).into()).await?;
            }
            Ok(Some(frame)) = connection.read_frame() => {
                match frame.data {
                    ActionEnum::Up(frame) => data.lock().await.append(&frame, None, None).await?,
                    ActionEnum::Down(frame) => data.lock().await.remove(&frame).await?,
                    ActionEnum::Sync(frame) => {
                        eprintln!("Sync {:#?}", frame);
                       match frame.clone() {
                            luoshu::data::LuoshuSyncDataEnum::LuoshuData(LuoshuDataEnum::Configuration(config))=>callback(config),
                           _ => println!("Sync {:#?}", frame),
                       };
                        data.lock().await.sync(&frame).await?;
                    },
                    _ => {}
                }
            }
            true = time_sleep()=>{
                connection.write_frame(&ActionEnum::Ping.into()).await?;
            }
            _ = signal::ctrl_c() => {
                break Ok(());
            }
            // 监听程序被中断
            _ = terminate => {
                break Ok(());
            }
        }
    }
}

/// A Python module implemented in Rust.
#[pymodule]
fn luoshu_python_sdk(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Luoshu>()?;
    Ok(())
}
