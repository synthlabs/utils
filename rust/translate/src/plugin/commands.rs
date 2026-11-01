use std::sync::Arc;

use tauri::{Runtime, State, Webview};

use super::{dto::*, service::Owner, ServiceError, TranslationService};

type Service<'a> = State<'a, Arc<TranslationService>>;

fn owner<R: Runtime>(webview: &Webview<R>) -> Owner {
    Owner::Webview(webview.window().label().into())
}

#[tauri::command]
fn status(service: Service<'_>) -> ServiceSnapshot {
    service.snapshot()
}

#[tauri::command]
async fn retry_initialize(service: Service<'_>) -> Result<ServiceSnapshot, ServiceError> {
    service.inner().initialize().await
}

#[tauri::command]
async fn release(service: Service<'_>) -> Result<ServiceSnapshot, ServiceError> {
    service.release().await
}

#[tauri::command]
async fn resume(service: Service<'_>) -> Result<ServiceSnapshot, ServiceError> {
    service.resume().await
}

#[tauri::command]
async fn detect(text: String, service: Service<'_>) -> Result<Option<DetectionDto>, ServiceError> {
    Ok(service.detect(text).await?.map(Into::into))
}

#[tauri::command]
async fn submit_translation<R: Runtime>(
    webview: Webview<R>,
    request: RequestDto,
    service: Service<'_>,
) -> Result<String, ServiceError> {
    service.submit_owned(owner(&webview), request.try_into()?)
}

#[tauri::command]
async fn translation_result<R: Runtime>(
    webview: Webview<R>,
    id: String,
    service: Service<'_>,
) -> Result<OutcomeDto, ServiceError> {
    Ok(service.result_owned(&owner(&webview), &id).await?.into())
}

#[tauri::command]
fn cancel_translation<R: Runtime>(
    webview: Webview<R>,
    id: String,
    service: Service<'_>,
) -> Result<(), ServiceError> {
    service.cancel_owned(&owner(&webview), &id)
}

#[tauri::command]
fn catalog(service: Service<'_>) -> Result<CatalogStateDto, ServiceError> {
    service.catalog()
}

#[tauri::command]
async fn refresh_catalog(service: Service<'_>) -> Result<CatalogStateDto, ServiceError> {
    service.refresh_catalog().await
}

#[tauri::command]
async fn inventory(service: Service<'_>) -> Result<Vec<InventoryDto>, ServiceError> {
    service.inventory().await
}

#[tauri::command]
async fn verify_model(pair: PairDto, service: Service<'_>) -> Result<ModelDto, ServiceError> {
    service.verify_model(pair.try_into()?).await
}

#[tauri::command]
async fn remove_model(pair: PairDto, service: Service<'_>) -> Result<bool, ServiceError> {
    service.remove_model(pair.try_into()?).await
}

#[tauri::command]
async fn discard_downloads(pair: PairDto, service: Service<'_>) -> Result<bool, ServiceError> {
    service.discard_downloads(pair.try_into()?).await
}

#[tauri::command]
async fn plan_install(pair: PairDto, service: Service<'_>) -> Result<InstallPlanDto, ServiceError> {
    service.plan_install(pair.try_into()?).await
}

#[tauri::command]
async fn start_install(
    plan: InstallPlanDto,
    service: Service<'_>,
) -> Result<InstallationDto, ServiceError> {
    service.inner().start_install(plan).await
}

#[tauri::command]
fn installation(id: String, service: Service<'_>) -> Result<InstallationDto, ServiceError> {
    service.installation(&id)
}

#[tauri::command]
fn cancel_install(id: String, service: Service<'_>) -> Result<(), ServiceError> {
    service.cancel_install(&id)
}

pub(super) fn handler<R: Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static
{
    macro_rules! handlers {
        ($($command:ident),* $(,)?) => { tauri::generate_handler![$($command),*] };
    }
    translate_build::translation_commands!(handlers)
}
