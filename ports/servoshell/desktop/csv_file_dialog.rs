use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

pub(crate) type PendingFile = Pin<Box<dyn Future<Output = Option<PathBuf>>>>;

pub(crate) fn take_picked(dialog: &mut Option<PendingFile>) -> Option<PathBuf> {
    let future = dialog.as_mut()?;
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(path) => {
            *dialog = None;
            path
        }
        Poll::Pending => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_path_is_consumed_once() {
        let mut dialog: Option<PendingFile> = Some(Box::pin(std::future::ready(Some(PathBuf::from("synthetic.csv")))));
        assert_eq!(take_picked(&mut dialog), Some(PathBuf::from("synthetic.csv")));
        assert!(dialog.is_none());
        assert_eq!(take_picked(&mut dialog), None);
    }

    #[test]
    fn cancellation_clears_pending_state() {
        let mut dialog: Option<PendingFile> = Some(Box::pin(std::future::ready(None)));
        assert_eq!(take_picked(&mut dialog), None);
        assert!(dialog.is_none());
    }

    #[test]
    fn pending_selection_is_retained() {
        let mut dialog: Option<PendingFile> = Some(Box::pin(std::future::pending()));
        assert_eq!(take_picked(&mut dialog), None);
        assert!(dialog.is_some());
    }
}
