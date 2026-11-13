use super::candidate::MentionType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SearchMode {
    Results,
    FilesystemOnly,
    Tools,
    Sessions,
}

impl SearchMode {
    pub(super) fn previous(self) -> Self {
        match self {
            Self::Results => Self::Sessions,
            Self::FilesystemOnly => Self::Results,
            Self::Tools => Self::FilesystemOnly,
            Self::Sessions => Self::Tools,
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            Self::Results => Self::FilesystemOnly,
            Self::FilesystemOnly => Self::Tools,
            Self::Tools => Self::Sessions,
            Self::Sessions => Self::Results,
        }
    }

    pub(super) fn accepts(self, mention_type: MentionType) -> bool {
        match self {
            Self::Results => true,
            Self::FilesystemOnly => {
                matches!(mention_type, MentionType::File | MentionType::Directory)
            }
            Self::Tools => matches!(mention_type, MentionType::Plugin | MentionType::Skill),
            Self::Sessions => mention_type == MentionType::Task,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Results => "All Results",
            Self::FilesystemOnly => "Filesystem Only",
            Self::Tools => "Plugins",
            Self::Sessions => "Sessions",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_filter_only_accepts_tasks() {
        for mention_type in [
            MentionType::Plugin,
            MentionType::Skill,
            MentionType::Task,
            MentionType::File,
            MentionType::Directory,
        ] {
            assert_eq!(
                SearchMode::Sessions.accepts(mention_type),
                mention_type == MentionType::Task
            );
            assert!(SearchMode::Results.accepts(mention_type));
        }
        assert!(SearchMode::Tools.accepts(MentionType::Plugin));
        assert!(SearchMode::Tools.accepts(MentionType::Skill));
        assert!(!SearchMode::Tools.accepts(MentionType::Task));
        assert_eq!(SearchMode::Tools.label(), "Plugins");
        assert_eq!(SearchMode::Sessions.label(), "Sessions");
    }

    #[test]
    fn filters_cycle_in_both_directions() {
        let modes = [
            SearchMode::Results,
            SearchMode::FilesystemOnly,
            SearchMode::Tools,
            SearchMode::Sessions,
        ];
        for (index, mode) in modes.into_iter().enumerate() {
            assert_eq!(mode.next(), modes[(index + 1) % modes.len()]);
            assert_eq!(
                mode.previous(),
                modes[(index + modes.len() - 1) % modes.len()]
            );
        }
    }
}
