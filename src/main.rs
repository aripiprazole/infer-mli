use std::fs::read_to_string;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::{LanguageServer, ServerSocket};
use clap::Parser;
use color_eyre::eyre::Context;
use futures::channel::oneshot;
use lsp_types::notification::{LogMessage, Progress, PublishDiagnostics, ShowMessage};
use lsp_types::request::Request;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentParams, DocumentFormattingParams, InitializeParams,
    InitializedParams, NumberOrString, ProgressParamsValue, TextDocumentItem, TextEdit, Url,
    WindowClientCapabilities, WorkDoneProgress, WorkspaceFolder,
};
use ropey::Rope;
use tower::ServiceBuilder;
use tracing::Level;

struct ClientState {
    indexed_tx: Option<oneshot::Sender<()>>,
}

struct Stop;

struct InferIntf;

impl Request for InferIntf {
    type Params = Vec<Url>;
    type Result = String;
    const METHOD: &'static str = "ocamllsp/inferIntf";
}

#[derive(clap::Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    root_dir: String,

    #[clap(short, long)]
    file: String,
}

async fn infer_intf(socket: &mut ServerSocket, file: &mut PathBuf) -> color_eyre::Result<String> {
    let url = Url::from_file_path(file.clone()).expect("file should be valid");
    let text = socket
        .request::<InferIntf>(vec![url])
        .await
        .wrap_err("couldn't infer interface")?;

    file.set_extension("mli");

    let mli_url = Url::from_file_path(file.clone()).expect("file should be valid");

    // open the mli file to be formatted
    open_file(socket, file.clone(), &text).await?;

    // format the mli file
    let format_result = socket
        .formatting(DocumentFormattingParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: mli_url },
            options: Default::default(),
            work_done_progress_params: Default::default(),
        })
        .await;

    // check if the formatting was successful
    if let Ok(result) = format_result {
        let mut rope = Rope::from_str(&text);
        apply_edits(&mut rope, &result.unwrap_or_default());
        Ok(rope.to_string())
    } else {
        Ok(text)
    }
}

async fn open_file(socket: &mut ServerSocket, file: PathBuf, text: &str) -> color_eyre::Result<()> {
    let url = Url::from_file_path(file).expect("file should be valid");
    socket
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: url.clone(),
                language_id: "ocaml".into(),
                version: 0,
                text: text.into(),
            },
        })
        .wrap_err("couldn't open file")?;
    Ok(())
}

fn apply_edits(text: &mut Rope, edits: &[TextEdit]) {
    for edit in edits {
        let start =
            text.line_to_byte(edit.range.start.line as usize) + edit.range.start.character as usize;
        let end =
            text.line_to_byte(edit.range.end.line as usize) + edit.range.end.character as usize;

        text.remove(start..end);
        text.insert(start, &edit.new_text);
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> color_eyre::Result<()> {
    let args = Args::parse();
    let root_dir = Path::new(&args.root_dir)
        .canonicalize()
        .expect("test root should be valid");

    let mut real_file = root_dir.join(&args.file);
    let text = read_to_string(&real_file).wrap_err("couldn't read file")?;

    let (indexed_tx, _) = oneshot::channel();

    let (mainloop, mut server) = async_lsp::MainLoop::new_client(|_server| {
        let mut router = Router::new(ClientState {
            indexed_tx: Some(indexed_tx),
        });
        router
            .notification::<Progress>(|this, prog| {
                tracing::debug!("{:?} {:?}", prog.token, prog.value);
                if matches!(prog.token, NumberOrString::String(_))
                    && matches!(
                        prog.value,
                        ProgressParamsValue::WorkDone(WorkDoneProgress::End(_))
                    )
                {
                    // Sometimes rust-analyzer auto-index multiple times?
                    if let Some(tx) = this.indexed_tx.take() {
                        let _: Result<_, _> = tx.send(());
                    }
                }
                ControlFlow::Continue(())
            })
            .notification::<PublishDiagnostics>(|_, _| ControlFlow::Continue(()))
            .notification::<ShowMessage>(|_, params| {
                tracing::debug!("show message: {:?}: {}", params.typ, params.message);
                ControlFlow::Continue(())
            })
            .notification::<LogMessage>(|_, params| {
                tracing::debug!("log message: {:?}: {}", params.typ, params.message);
                ControlFlow::Continue(())
            })
            .event(|_, _: Stop| ControlFlow::Break(Ok(())));

        ServiceBuilder::new()
            .layer(CatchUnwindLayer::default())
            .layer(ConcurrencyLayer::default())
            .service(router)
    });

    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();

    let child = async_process::Command::new("ocamllsp")
        .current_dir(&root_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("Failed run rust-analyzer");
    let stdout = child.stdout.unwrap();
    let stdin = child.stdin.unwrap();

    let mainloop_fut = tokio::spawn(async move {
        mainloop.run_buffered(stdout, stdin).await.unwrap();
    });

    // Initialize.
    server
        .initialize(InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&root_dir).unwrap(),
                name: "root".into(),
            }]),
            capabilities: ClientCapabilities {
                window: Some(WindowClientCapabilities {
                    work_done_progress: Some(true),
                    ..WindowClientCapabilities::default()
                }),
                ..ClientCapabilities::default()
            },
            ..InitializeParams::default()
        })
        .await
        .wrap_err("couldn't initialize")?;

    server
        .initialized(InitializedParams {})
        .wrap_err("couldn't initialize")?;

    open_file(&mut server, real_file.clone(), &text).await?;

    let Ok(text) = infer_intf(&mut server, &mut real_file).await else {
        // Shutdown.
        server.shutdown(()).await.wrap_err("couldn't shutdown")?;
        server.exit(()).wrap_err("couldn't exit")?;

        server.emit(Stop).wrap_err("couldn't emit stop event")?;
        mainloop_fut.await.wrap_err("couldn't finish main loop")?;

        return Ok(());
    };

    std::fs::write(&real_file, text).wrap_err("couldn't write file")?;
    println!("{}", real_file.to_string_lossy());

    // Shutdown.
    server.shutdown(()).await.wrap_err("couldn't shutdown")?;
    server.exit(()).wrap_err("couldn't exit")?;

    server.emit(Stop).wrap_err("couldn't emit stop event")?;
    mainloop_fut.await.wrap_err("couldn't finish main loop")?;
    Ok(())
}
