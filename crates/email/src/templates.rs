//! Email templates: each message is a pair `<name>.html.j2` + `<name>.txt.j2`.
//! The text template carries the one-line `subject` block. HTML is auto-escaped.
use minijinja::{AutoEscape, Environment};
use serde_json::{json, Value};

/// `(file name, source)` for one embedded template file.
macro_rules! template {
    ($file:literal) => {
        ($file, include_str!(concat!("../templates/", $file)))
    };
}

/// Every embedded template. Each message needs a `.html.j2` and a `.txt.j2` (the guard test
/// `every_template_file_on_disk_is_registered` fails if one is missing here).
const SOURCES: &[(&str, &str)] = &[
    template!("layout.html.j2"),
    template!("layout.txt.j2"),
    template!("approval_reminder.html.j2"),
    template!("approval_reminder.txt.j2"),
    template!("cancel_declined.html.j2"),
    template!("comp_off_decided.html.j2"),
    template!("comp_off_decided.txt.j2"),
    template!("comp_off_expiring.html.j2"),
    template!("comp_off_expiring.txt.j2"),
    template!("comp_off_fyi.html.j2"),
    template!("comp_off_fyi.txt.j2"),
    template!("comp_off_requested_action.html.j2"),
    template!("comp_off_requested_action.txt.j2"),
    template!("cancel_declined.txt.j2"),
    template!("cancel_requested.html.j2"),
    template!("cancel_requested.txt.j2"),
    template!("holiday_changed_leave.html.j2"),
    template!("holiday_changed_leave.txt.j2"),
    template!("invite.html.j2"),
    template!("invite.txt.j2"),
    template!("leave_cancelled.html.j2"),
    template!("leave_cancelled.txt.j2"),
    template!("leave_decided.html.j2"),
    template!("leave_decided.txt.j2"),
    template!("leave_decided_fyi.html.j2"),
    template!("leave_decided_fyi.txt.j2"),
    template!("leave_requested_action.html.j2"),
    template!("leave_requested_action.txt.j2"),
    template!("leave_requested_confirm.html.j2"),
    template!("leave_requested_confirm.txt.j2"),
    template!("leave_requested_fyi.html.j2"),
    template!("leave_requested_fyi.txt.j2"),
    template!("login_code.html.j2"),
    template!("login_code.txt.j2"),
    template!("rollover_admin_summary.html.j2"),
    template!("rollover_admin_summary.txt.j2"),
    template!("rollover_employee.html.j2"),
    template!("rollover_employee.txt.j2"),
    template!("test_email.html.j2"),
    template!("test_email.txt.j2"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub subject: String,
    pub html: String,
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("unknown email template {0}")]
    Unknown(String),
    #[error("template error: {0}")]
    Render(#[from] minijinja::Error),
}

pub struct Templates {
    env: Environment<'static>,
}

impl Default for Templates {
    fn default() -> Self {
        Self::new()
    }
}

impl Templates {
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_auto_escape_callback(|name| {
            if name.contains(".html") {
                AutoEscape::Html
            } else {
                AutoEscape::None
            }
        });
        for (name, src) in SOURCES {
            env.add_template(name, src)
                .expect("embedded email template must parse");
        }
        Self { env }
    }

    /// Message names (excluding layouts).
    pub fn names(&self) -> Vec<String> {
        SOURCES
            .iter()
            .filter_map(|(n, _)| n.strip_suffix(".txt.j2"))
            .filter(|n| !n.starts_with("layout"))
            .map(str::to_string)
            .collect()
    }

    pub fn render(&self, name: &str, ctx: &Value) -> Result<Rendered, TemplateError> {
        let lookup = |file: String| {
            self.env.get_template(&file).map_err(|e| match e.kind() {
                minijinja::ErrorKind::TemplateNotFound => TemplateError::Unknown(name.to_string()),
                _ => TemplateError::Render(e),
            })
        };
        let txt = lookup(format!("{name}.txt.j2"))?;
        let html = lookup(format!("{name}.html.j2"))?;
        let ctx = minijinja::Value::from(minijinja::value::Serde(ctx));
        let mut captured = txt.render_captured(&ctx)?;
        let subject = captured.with_state_mut(|state| state.render_block("subject"))?;
        Ok(Rendered {
            subject: subject.split_whitespace().collect::<Vec<_>>().join(" "),
            text: captured.into_output().trim().to_string() + "\n",
            html: html.render(&ctx)?,
        })
    }

    /// Example context for previews and template tests. Templates added by later stories
    /// extend this with the fields they use.
    pub fn sample_context(name: &str) -> Value {
        let mut ctx = json!({ "org_name": "Acme", "base_url": "https://leave.acme.test" });
        let request = json!({
            "requester": "Asha Rao",
            "dates": "24 Dec 2026 – 29 Dec 2026 (first half)",
            "days_text": "2.5 days",
            "status": "pending",
        });
        match name {
            "login_code" => {
                ctx["code"] = json!("042917");
                ctx["purpose"] = json!("login");
            }
            "leave_requested_confirm" | "leave_requested_action" => {
                merge(&mut ctx, &request);
                ctx["leave_type"] = json!("Casual Leave");
                ctx["reason"] = json!("Family trip");
                ctx["has_attachment"] = json!(true);
                ctx["approve_url"] = json!("https://leave.acme.test/approvals?decision=approve");
                ctx["reject_url"] = json!("https://leave.acme.test/approvals?decision=reject");
            }
            "leave_decided" => {
                merge(&mut ctx, &request);
                ctx["leave_type"] = json!("Casual Leave");
                ctx["status"] = json!("approved");
                ctx["decided_by"] = json!("Priya Shah");
                ctx["comment"] = json!("Enjoy!");
            }
            "comp_off_requested_action" => {
                merge(&mut ctx, &request);
                ctx["dates"] = json!("Sat 21 Nov 2026, Sun 22 Nov 2026 (half day)");
                ctx["days_text"] = json!("1.5 days");
                ctx["reason"] = json!("Release weekend");
                ctx["approve_url"] = json!("https://leave.acme.test/approvals?decision=approve");
                ctx["reject_url"] = json!("https://leave.acme.test/approvals?decision=reject");
            }
            "comp_off_decided" => {
                merge(&mut ctx, &request);
                ctx["status"] = json!("approved");
                ctx["decided_by"] = json!("Priya Shah");
                ctx["comment"] = json!("Thanks for covering");
                ctx["expiry_days"] = json!(90);
            }
            "comp_off_fyi" => {
                merge(&mut ctx, &request);
                ctx.as_object_mut().unwrap().remove("base_url");
            }
            "comp_off_expiring" => {
                ctx["requester"] = json!("Asha Rao");
                ctx["days_text"] = json!("1 day");
                ctx["worked_date"] = json!("21 Nov 2026");
                ctx["expires_on"] = json!("19 Feb 2027");
            }
            "invite" => {
                ctx["full_name"] = json!("Asha Rao");
                ctx["invited_by"] = json!("Priya Shah");
                ctx["login_url"] = json!("https://leave.acme.test/login");
            }
            "rollover_employee" => {
                ctx["requester"] = json!("Asha Rao");
                ctx["year"] = json!(2027);
                ctx["lines"] = json!(["Casual Leave: 17 days (12 allotted, 5 carried, 3 lapsed)"]);
            }
            "rollover_admin_summary" => {
                ctx["year"] = json!(2027);
                ctx["closing_year"] = json!(2026);
                ctx["employees"] = json!(42);
                ctx["carried"] = json!("120");
                ctx["lapsed"] = json!("37.5");
            }
            "holiday_changed_leave" => {
                merge(&mut ctx, &request);
                ctx["leave_type"] = json!("Casual Leave");
                ctx["holiday_date"] = json!("Fri 25 Dec 2026");
                ctx["days_text"] = json!("1 day");
                ctx["booked"] = json!(true);
                ctx["to_requester"] = json!(true);
            }
            "leave_cancelled" => {
                merge(&mut ctx, &request);
                ctx["leave_type"] = json!("Casual Leave");
                ctx["to_requester"] = json!(true);
                ctx["accepted_by"] = json!("Priya Shah");
                ctx["returned_text"] = json!("1.5 days");
                ctx["kept_text"] = json!("1 day");
                ctx["reason"] = json!("Back early");
            }
            "cancel_requested" => {
                merge(&mut ctx, &request);
                ctx["leave_type"] = json!("Casual Leave");
                ctx["returned_text"] = json!("1.5 days");
                ctx["requested_on"] = json!("Mon 28 Dec");
                ctx["reason"] = json!("Back early");
                ctx["inbox_url"] = json!("https://leave.acme.test/approvals");
            }
            "cancel_declined" => {
                merge(&mut ctx, &request);
                ctx["leave_type"] = json!("Casual Leave");
                ctx["decided_by"] = json!("Priya Shah");
                ctx["comment"] = json!("Release is this week");
            }
            "approval_reminder" => {
                merge(&mut ctx, &request);
                ctx["leave_type"] = json!("Casual Leave");
                ctx["reason"] = json!("Family trip");
                ctx["waiting"] = json!("2 days");
                ctx["inbox_url"] = json!("https://leave.acme.test/approvals?request=1");
            }
            // FYI emails carry only name, dates, days and status (FR-059).
            "leave_requested_fyi" | "leave_decided_fyi" => {
                merge(&mut ctx, &request);
                ctx.as_object_mut().unwrap().remove("base_url");
            }
            _ => {}
        }
        ctx
    }
}

fn merge(into: &mut Value, from: &Value) {
    if let (Some(a), Some(b)) = (into.as_object_mut(), from.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
}
