use std::fmt::{self, Write};
use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
    level_filters::LevelFilter,
    span,
    subscriber::Interest,
};

pub(super) fn install() {
    let _ = tracing::subscriber::set_global_default(RelayLogger {
        filter: LogFilter::from_env(),
    });
}

#[derive(Debug)]
struct RelayLogger {
    filter: LogFilter,
}

impl Subscriber for RelayLogger {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.filter.allows(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.filter.allows(metadata)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        self.filter.max_level()
    }

    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }

    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}

    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}

    fn event(&self, event: &Event<'_>) {
        if !self.filter.allows(event.metadata()) {
            return;
        }
        let mut fields = EventFields::default();
        event.record(&mut fields);
        if fields.text.is_empty() {
            eprintln!("{} {}", event.metadata().level(), event.metadata().name());
        } else {
            eprintln!("{} {}", event.metadata().level(), fields.text);
        }
    }

    fn enter(&self, _: &span::Id) {}

    fn exit(&self, _: &span::Id) {}
}

#[derive(Default)]
struct EventFields {
    text: String,
}

impl Visit for EventFields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        let _ = write!(self.text, "{}={value:?}", field.name());
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LogFilter {
    pub(crate) default: LevelFilter,
    pub(crate) targets: Vec<(String, LevelFilter)>,
}

impl LogFilter {
    fn from_env() -> Self {
        let mut filter = Self {
            default: LevelFilter::INFO,
            targets: Vec::new(),
        };
        let Ok(value) = std::env::var("RUST_LOG") else {
            return filter;
        };
        for directive in value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let (target, level) = match directive.rsplit_once('=') {
                Some((target, level)) => (Some(target.trim()), level.trim()),
                None => (None, directive),
            };
            let Ok(level) = level.parse::<LevelFilter>() else {
                continue;
            };
            match target {
                Some(target) if !target.is_empty() => {
                    filter.targets.push((target.to_string(), level))
                }
                _ => filter.default = level,
            }
        }
        filter
    }

    fn allows(&self, metadata: &Metadata<'_>) -> bool {
        let level = self
            .targets
            .iter()
            .filter(|(target, _)| {
                metadata.target() == target || metadata.target().starts_with(&format!("{target}::"))
            })
            .max_by_key(|(target, _)| target.len())
            .map(|(_, level)| *level)
            .unwrap_or(self.default);
        Self::allows_level(level, *metadata.level())
    }

    fn max_level(&self) -> Option<LevelFilter> {
        self.targets
            .iter()
            .map(|(_, level)| *level)
            .chain(std::iter::once(self.default))
            .max()
    }

    pub(crate) fn allows_level(level: LevelFilter, message_level: Level) -> bool {
        match (level, message_level) {
            (LevelFilter::OFF, _) => false,
            (LevelFilter::ERROR, Level::ERROR) => true,
            (LevelFilter::WARN, Level::ERROR | Level::WARN) => true,
            (LevelFilter::INFO, Level::ERROR | Level::WARN | Level::INFO) => true,
            (LevelFilter::DEBUG, Level::ERROR | Level::WARN | Level::INFO | Level::DEBUG) => true,
            (LevelFilter::TRACE, _) => true,
            _ => false,
        }
    }
}
