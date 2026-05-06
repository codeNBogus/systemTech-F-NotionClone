//! Write-Ahead Log: 모든 상태 변경 이벤트를 파일에 append-only로 기록하고
//! 서버 시작 시 replay하여 메모리 상태를 복구한다.

use crate::models::WsEvent;
use std::fs::{File, OpenOptions, create_dir_all};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// WAL 파일에 이벤트를 append하는 writer.
/// 내부적으로 Mutex<File>로 동시 write 방지.
pub struct WalWriter {
    file: Mutex<File>,
    path: PathBuf,
}

impl WalWriter {
    /// 지정 경로에 WAL 파일을 열거나 새로 생성.
    /// 부모 디렉토리가 없으면 자동 생성.
    pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            file: Mutex::new(file),
            path,
        })
    }

    /// 이벤트 한 건을 JSON-Lines 형식으로 append + fsync.
    /// fsync까지 호출해야 crash 후에도 데이터가 살아남는다.
    pub fn append(&self, event: &WsEvent) -> std::io::Result<()> {
        let json = serde_json::to_string(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut file = self.file.lock().unwrap();
        writeln!(file, "{}", json)?;
        file.sync_all()?; // fsync — 디스크에 실제로 기록될 때까지 대기
        Ok(())
    }

    /// WAL 파일을 처음부터 읽어 이벤트 목록으로 복원.
    /// 마지막 줄이 손상된 경우(torn write) 무시하고 정상 복구.
    pub fn replay<P: AsRef<Path>>(path: P) -> std::io::Result<Vec<WsEvent>> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for (idx, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<WsEvent>(&line) {
                Ok(ev) => events.push(ev),
                Err(e) => {
                    eprintln!(
                        "⚠️  WAL replay: line {} corrupted, stopping replay: {}",
                        idx + 1,
                        e
                    );
                    break; // torn write — 마지막 손상된 줄 이후는 무시
                }
            }
        }
        Ok(events)
    }
}
