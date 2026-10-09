//! 编辑历史：撤销 / 重做 / 跳转 / 快照。

use serde::{Deserialize, Serialize};

use crate::params::EditParams;

/// 历史记录上限（超出后丢弃最旧的非初始项）。
pub const DEFAULT_HISTORY_LIMIT: usize = 200;

/// 一条历史记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// 操作名称（显示在历史面板）。
    pub label: String,
    /// 该步骤的参数快照。
    pub params: EditParams,
    /// 是否为手动快照。
    pub snapshot: bool,
}

/// 线性历史栈。
#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<HistoryEntry>,
    index: usize,
    limit: usize,
}

impl History {
    /// 使用初始状态创建历史。
    pub fn new(label: impl Into<String>, params: EditParams) -> Self {
        Self {
            entries: vec![HistoryEntry {
                label: label.into(),
                params,
                snapshot: false,
            }],
            index: 0,
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }

    /// 设置历史长度上限。
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    /// 追加一步（丢弃当前位置之后的重做分支）。
    pub fn push(&mut self, label: impl Into<String>, params: EditParams) {
        self.entries.truncate(self.index + 1);
        self.entries.push(HistoryEntry {
            label: label.into(),
            params,
            snapshot: false,
        });
        if self.entries.len() > self.limit {
            let overflow = self.entries.len() - self.limit;
            self.entries.drain(0..overflow);
        }
        self.index = self.entries.len() - 1;
    }

    /// 记录快照（不改变当前位置）。
    pub fn snapshot(&mut self, label: impl Into<String>, params: EditParams) {
        self.push(label, params);
        if let Some(entry) = self.entries.get_mut(self.index) {
            entry.snapshot = true;
        }
    }

    /// 撤销。
    pub fn undo(&mut self) -> Option<&EditParams> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        self.entries.get(self.index).map(|e| &e.params)
    }

    /// 重做。
    pub fn redo(&mut self) -> Option<&EditParams> {
        if self.index + 1 >= self.entries.len() {
            return None;
        }
        self.index += 1;
        self.entries.get(self.index).map(|e| &e.params)
    }

    /// 跳转到指定步骤，返回该步骤参数。
    pub fn jump_to(&mut self, index: usize) -> Option<&EditParams> {
        if index >= self.entries.len() {
            return None;
        }
        self.index = index;
        self.entries.get(self.index).map(|e| &e.params)
    }

    /// 全部历史步骤。
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// 当前步骤下标。
    pub fn index(&self) -> usize {
        self.index
    }

    /// 步骤数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空（初始项始终存在，仅在极端情况下为真）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_and_jump() {
        let mut history = History::new("打开图片", EditParams::default());
        let mut second = EditParams::default();
        second.light.exposure = 1.0;
        history.push("曝光", second.clone());
        let mut third = second.clone();
        third.light.contrast = 20.0;
        history.push("对比度", third.clone());

        assert_eq!(history.len(), 3);
        assert_eq!(history.undo().unwrap().light.exposure, 1.0);
        assert_eq!(history.redo().unwrap().light.contrast, 20.0);
        assert_eq!(history.jump_to(0).unwrap().light.exposure, 0.0);
        assert_eq!(history.jump_to(99), None);
    }

    #[test]
    fn push_truncates_redo_branch() {
        let mut history = History::new("打开图片", EditParams::default());
        history.push("a", EditParams::default());
        history.push("b", EditParams::default());
        history.undo();
        history.push("c", EditParams::default());
        assert_eq!(history.len(), 3);
        assert_eq!(history.entries()[2].label, "c");
    }
}
