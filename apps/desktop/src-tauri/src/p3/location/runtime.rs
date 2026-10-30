//! Optional system WinRT activation. Unsupported location never prevents startup.
use super::Fix;
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{imp::IGenericFactory, Interface, GUID, HRESULT, HSTRING},
    Devices::Geolocation::{
        GeolocationAccessStatus, Geolocator, IGeolocatorStatics, PositionStatus,
    },
    Foundation::{AsyncOperationCompletedHandler, IAsyncOperation, TimeSpan},
    Win32::{
        Foundation::HMODULE,
        System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
    },
};
static COMBASE: OnceLock<Option<usize>> = OnceLock::new();
static MTA: OnceLock<Option<usize>> = OnceLock::new();
fn factory<T: Interface>(class: &str) -> Result<T, String> {
    let module = COMBASE
        .get_or_init(|| unsafe {
            LoadLibraryExW(
                windows::core::w!("combase.dll"),
                None,
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
            .ok()
            .map(|m| m.0 as usize)
        })
        .map(|p| HMODULE(p as *mut c_void))
        .ok_or("Windows 位置接口不可用")?;
    MTA.get_or_init(|| unsafe {
        let pointer = GetProcAddress(module, windows::core::s!("CoIncrementMTAUsage"))?;
        let call: unsafe extern "system" fn(*mut *mut c_void) -> HRESULT =
            std::mem::transmute(pointer);
        let mut cookie = std::ptr::null_mut();
        call(&mut cookie).ok().ok()?;
        Some(cookie as usize)
    });
    unsafe {
        let pointer = GetProcAddress(module, windows::core::s!("RoGetActivationFactory"))
            .ok_or("Windows 位置接口不可用")?;
        let call: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT =
            std::mem::transmute(pointer);
        let name = HSTRING::from(class);
        let mut result = std::ptr::null_mut();
        call(std::mem::transmute_copy(&name), &T::IID, &mut result)
            .ok()
            .map_err(|_| "Windows 位置接口不可用")?;
        if result.is_null() {
            return Err("Windows 位置接口不可用".into());
        }
        Ok(T::from_raw(result))
    }
}
pub fn access() -> &'static str {
    use windows::Security::Authorization::AppCapabilityAccess::{
        AppCapability, AppCapabilityAccessStatus as S, IAppCapabilityStatics,
    };
    let result = (|| -> Result<S, String> {
        let statics = factory::<IAppCapabilityStatics>(
            "Windows.Security.Authorization.AppCapabilityAccess.AppCapability",
        )?;
        unsafe {
            let name = HSTRING::from("location");
            let mut result = std::ptr::null_mut();
            (Interface::vtable(&statics).Create)(
                statics.as_raw(),
                std::mem::transmute_copy(&name),
                &mut result,
            )
            .ok()
            .map_err(|_| "位置权限不可用")?;
            if result.is_null() {
                return Err("位置权限不可用".into());
            }
            AppCapability::from_raw(result)
                .CheckAccess()
                .map_err(|_| "位置权限不可用".into())
        }
    })();
    match result {
        Ok(S::Allowed) => "allowed",
        Ok(S::DeniedByUser | S::DeniedBySystem) => "denied",
        Ok(S::UserPromptRequired) => "not-requested",
        _ => "unavailable",
    }
}
pub fn request_access() -> Result<IAsyncOperation<GeolocationAccessStatus>, String> {
    let statics = factory::<IGeolocatorStatics>("Windows.Devices.Geolocation.Geolocator")?;
    unsafe {
        let mut result = std::ptr::null_mut();
        (Interface::vtable(&statics).RequestAccessAsync)(statics.as_raw(), &mut result)
            .ok()
            .map_err(|_| "Windows 定位申请不可用")?;
        if result.is_null() {
            return Err("Windows 定位申请不可用".into());
        }
        Ok(IAsyncOperation::from_raw(result))
    }
}
pub fn wait<T: windows::core::RuntimeType + Send + 'static>(
    op: IAsyncOperation<T>,
    seconds: u64,
    cancel: &AtomicBool,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let copy = op.clone();
    op.SetCompleted(&AsyncOperationCompletedHandler::new(move |_, _| {
        let _ = tx.try_send(
            copy.GetResults()
                .map_err(|_| "Windows 位置服务不可用或权限被拒绝".to_string()),
        );
        Ok(())
    }))
    .map_err(|_| "Windows 位置读取未开始")?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            let _ = op.Cancel();
            return Err("Windows 位置读取已取消或超时".into());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(v) => return v,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("Windows 位置读取未完成".into()),
        }
    }
}
pub fn read(cancel: &AtomicBool) -> Result<Fix, String> {
    let locator: Geolocator = factory::<IGenericFactory>("Windows.Devices.Geolocation.Geolocator")?
        .ActivateInstance()
        .map_err(|_| "Windows 位置接口不可用")?;
    if locator
        .LocationStatus()
        .map_err(|_| "Windows 位置状态不可用")?
        == PositionStatus::Disabled
    {
        return Err("Windows 定位服务已关闭".into());
    }
    let op = locator
        .GetGeopositionAsyncWithAgeAndTimeout(
            TimeSpan {
                Duration: 300 * 10_000_000,
            },
            TimeSpan {
                Duration: 10 * 10_000_000,
            },
        )
        .map_err(|_| "Windows 位置读取失败")?;
    let position = wait(op, 12, cancel)?;
    let coordinate = position.Coordinate().map_err(|_| "Windows 位置无效")?;
    let point = coordinate
        .Point()
        .and_then(|p| p.Position())
        .map_err(|_| "Windows 位置坐标无效")?;
    let measured_at = chrono::DateTime::from_timestamp(
        coordinate
            .Timestamp()
            .map_err(|_| "Windows 位置时间无效")?
            .UniversalTime
            / 10_000_000
            - 11_644_473_600,
        0,
    )
    .ok_or("Windows 位置时间无效")?;
    let fix = Fix {
        latitude: point.Latitude,
        longitude: point.Longitude,
        accuracy_meters: coordinate.Accuracy().map_err(|_| "Windows 位置精度无效")?,
        measured_at,
    };
    fix.validate(chrono::Utc::now())?;
    Ok(fix)
}
