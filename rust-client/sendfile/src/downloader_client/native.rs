use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{pin_mut, AsyncWrite, TryStreamExt};
use instant::Duration;
use tokio::fs::File;
use tokio::io::BufWriter;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use super::DownloaderClient;
use crate::util::native::current_thread_block_on;
use crate::util::ProgressState;
use crate::{api_client::DownloadMeta, Result};

struct DownloadFile {
    file_writer: tokio_util::compat::Compat<BufWriter<File>>,
}

impl DownloaderClient {
    pub fn fetch_meta(&self) -> Result<DownloadMeta> {
        current_thread_block_on(self.fetch_meta_async())
    }

    pub fn download<F: FnMut(ProgressState<u64>)>(
        &self,
        meta: &DownloadMeta,
        output_dir: Option<PathBuf>,
        p2p_timeout: Option<Duration>,
        mut progress_fun: F,
    ) -> Result<()> {
        current_thread_block_on(async {
            let file = DownloadFile::new(meta, output_dir.as_deref()).await?;
            let decrypted_file = self.api_client.decrypt_file(meta, file);
            let download_progress = self.download_async(meta, decrypted_file, p2p_timeout);
            pin_mut!(download_progress);
            while let Some(progress_state) = download_progress.try_next().await? {
                progress_fun(progress_state);
            }
            Ok(())
        })
    }
}

impl DownloadFile {
    async fn new(download_meta: &DownloadMeta, output_dir: Option<&Path>) -> io::Result<Self> {
        let output_path: PathBuf = if let Some(output_dir) = output_dir {
            Path::join(output_dir, &download_meta.file_meta.file_name)
        } else {
            PathBuf::from(&download_meta.file_meta.file_name)
        };
        let file = File::create(&output_path).await?;
        let file_writer = BufWriter::new(file).compat_write();
        Ok(Self { file_writer })
    }
}

impl AsyncWrite for DownloadFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.file_writer).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.file_writer).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.file_writer).poll_close(cx)
    }
}
