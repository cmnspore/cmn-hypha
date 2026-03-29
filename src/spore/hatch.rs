use std::process::ExitCode;

use super::{load_draft, save_draft};
use crate::api::Output;
use crate::auth;

pub struct HatchArgs {
    pub id: Option<String>,
    pub version: Option<String>,
    pub name: Option<String>,
    pub domain: Option<String>,
    pub synopsis: Option<String>,
    pub intent: Vec<String>,
    pub mutations: Vec<String>,
    pub license: Option<String>,
}

pub fn handle_hatch(out: &Output, args: HatchArgs) -> ExitCode {
    let (spore_core_path, mut draft) = match load_draft() {
        Ok(v) => v,
        Err((code, msg)) => return out.error(&code, &msg),
    };

    if let Some(i) = args.id {
        draft.id = i;
    }
    if let Some(v) = args.version {
        draft.version = v;
    }
    if let Some(n) = args.name {
        draft.name = n;
    }
    if let Some(d) = args.domain {
        // Resolve key from domain's keypair
        let site = crate::site::SiteDir::new(&d);
        match auth::get_identity_with_site(&d, &site) {
            Ok(info) => draft.key = info.public_key,
            Err(e) => {
                return out.error(
                    "identity_error",
                    &format!("Cannot resolve key for domain '{}': {}", d, e),
                )
            }
        }
        draft.domain = d;
    }
    if let Some(s) = args.synopsis {
        draft.synopsis = s;
    }
    if !args.intent.is_empty() {
        draft.intent = args.intent;
    }
    if !args.mutations.is_empty() {
        draft.mutations = args.mutations;
    }
    if let Some(l) = args.license {
        draft.license = l;
    }

    if let Err(e) = save_draft(&spore_core_path, &draft) {
        return out.error_hypha(&e);
    }

    out.ok(&draft)
}

pub fn handle_tree_set(
    out: &Output,
    algorithm: Option<String>,
    exclude_names: Option<Vec<String>>,
    follow_rules: Option<Vec<String>>,
) -> ExitCode {
    let (spore_core_path, mut draft) = match load_draft() {
        Ok(v) => v,
        Err((code, msg)) => return out.error(&code, &msg),
    };

    if let Some(a) = algorithm {
        draft.tree.algorithm = a;
    }
    if let Some(e) = exclude_names {
        draft.tree.exclude_names = e;
    }
    if let Some(f) = follow_rules {
        draft.tree.follow_rules = f;
    }

    if let Err(e) = save_draft(&spore_core_path, &draft) {
        return out.error_hypha(&e);
    }

    out.ok(&draft.tree)
}

pub fn handle_tree_show(out: &Output) -> ExitCode {
    let (_spore_core_path, draft) = match load_draft() {
        Ok(v) => v,
        Err((code, msg)) => return out.error(&code, &msg),
    };

    out.ok(&draft.tree)
}
