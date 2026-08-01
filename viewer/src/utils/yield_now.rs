//! 让出 UI 控制权的辅助工具。
//!
//! 说明：`poll-promise` 的 `tick_local()` 使用 `smol::LocalExecutor::try_tick()`，
//! 其语义是"poll 所有就绪任务直到没有立即可推进的任务"。这意味着如果让出时使用
//! `wake_by_ref()`（旧实现），任务会在同一次 try_tick 内被立即重新 poll，
//! 导致"让出"失效、长任务（如大表 CSV 导出）同步阻塞 UI 线程。
//!
//! 修复方式：使用跨线程 channel 作为"UI 帧节拍"。每帧由 `tick_promises`
//! 向 channel 发送一个通知；async 任务 `yield_to_ui().await` 时等待该通知。
//! channel 在无消息时挂起且**不会立即被唤醒**（只有收到 send 才 wake），
//! 因此 try_tick 会真正让出控制权，UI 每帧都能响应并绘制进度窗口。

use std::sync::OnceLock;

use async_channel::{Receiver, Sender};

/// 全局 UI 帧节拍 channel
fn ui_tick_channel() -> &'static (Sender<()>, Receiver<()>) {
    static CHANNEL: OnceLock<(Sender<()>, Receiver<()>)> = OnceLock::new();
    CHANNEL.get_or_init(|| async_channel::bounded::<()>(16))
}

/// 每帧由 App::update 调用（在 tick_promises 中），向等待中的 async 任务发一次通知。
/// channel 满时直接丢弃（任务稍后仍会收到下一帧的通知）。
pub fn notify_ui_tick() {
    let (tx, _) = ui_tick_channel();
    let _ = tx.try_send(());
}

/// 让出控制权到下一帧（等待一次 UI 帧节拍）。
pub async fn yield_to_ui() {
    let (_, rx) = ui_tick_channel();
    rx.recv().await.ok();
}

/// Future for the [`yield_now()`] function.
#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct YieldNow(bool);

impl std::future::Future for YieldNow {
    type Output = ();

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if !self.0 {
            self.0 = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    }
}

/// 立即让出一次（保留原有行为，供无需帧节拍的场景使用）。
pub fn yield_now() -> YieldNow {
    YieldNow(false)
}
