pub mod files;
pub mod git;
pub mod github;
pub mod origin;
pub mod processes;
pub mod settings;

use std::path::Path;

pub struct Shell {
    pub git: Box<dyn git::Git>,
    pub origin: Box<dyn origin::Origin>,
    pub github: Box<dyn github::GitHub>,
    pub files: Box<dyn files::StateFiles>,
    pub processes: Box<dyn processes::Processes>,
    pub settings: settings::Settings,
}

impl Shell {
    pub fn live(directory: &Path) -> Self {
        let settings = settings::Settings::from_env();
        Self {
            git: Box::new(git::Live::new(directory)),
            origin: Box::new(origin::Live::new(directory)),
            github: Box::new(github::Live),
            files: Box::new(files::Live::new(&settings.state_home)),
            processes: Box::new(processes::Live),
            settings,
        }
    }
}
