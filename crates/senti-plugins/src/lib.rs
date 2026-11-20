//! # senti-plugins
//!
//! Built-in [`senti_core::Plugin`] implementations. Each plugin can be
//! enabled or disabled at runtime through the engine's plugin registry.
//!
//! | id           | Plugin              | Trigger                                           |
//! |--------------|---------------------|---------------------------------------------------|
//! | `emoji`      | [`EmojiPlugin`]     | Top candidates matching a keyword (context aware) |
//! | `datetime`   | [`DateTimePlugin`]  | `rq` / `riqi`, `sj` / `shijian`, `xq`, `date`, `time` |
//! | `calculator` | [`CalculatorPlugin`]| Chinese mode input starting with `v`, e.g. `v1+2*3` |

mod calculator;
mod datetime;
mod emoji;

pub use calculator::{CalculatorPlugin, evaluate};
pub use datetime::DateTimePlugin;
pub use emoji::EmojiPlugin;

use senti_core::Plugin;

/// All built-in plugins, in display order.
pub fn builtin_plugins() -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(EmojiPlugin::default()),
        Box::new(DateTimePlugin),
        Box::new(CalculatorPlugin),
    ]
}

#[cfg(test)]
pub(crate) mod test_support {
    use senti_core::{InputContext, InputMode, PredictionQuery, UserModel};

    /// Build a query with owned context / user model for plugin tests.
    pub fn with_query<R>(
        input: &str,
        mode: InputMode,
        context: InputContext,
        f: impl FnOnce(&PredictionQuery<'_>) -> R,
    ) -> R {
        let user = UserModel::default();
        let query = PredictionQuery {
            input,
            mode,
            context: &context,
            previous: None,
            user: &user,
            limit: 10,
        };
        f(&query)
    }
}
