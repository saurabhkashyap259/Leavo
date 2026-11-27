//! The single error code catalogue (data-model.md §Error code catalogue).
//! The API returns only these codes; the web app maps them to translated messages.

macro_rules! error_codes {
    ($( $variant:ident = ($code:literal, $status:literal, $msg:literal) ),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ErrorCode { $( $variant ),+ }

        impl ErrorCode {
            pub const ALL: &'static [ErrorCode] = &[ $( ErrorCode::$variant ),+ ];

            pub fn as_str(self) -> &'static str {
                match self { $( ErrorCode::$variant => $code ),+ }
            }

            pub fn http_status(self) -> u16 {
                match self { $( ErrorCode::$variant => $status ),+ }
            }

            pub fn default_message(self) -> &'static str {
                match self { $( ErrorCode::$variant => $msg ),+ }
            }
        }
    };
}

error_codes! {
    // Domain rules
    InvalidRange = ("INVALID_RANGE", 422, "The end date is before the start date."),
    ZeroWorkingDays = ("ZERO_WORKING_DAYS", 422, "The selected dates contain no working days."),
    HalfDayNotAllowed = ("HALF_DAY_NOT_ALLOWED", 422, "This leave type does not allow half days."),
    InsufficientBalance = ("INSUFFICIENT_BALANCE", 422, "Not enough balance for this request."),
    NegativeLimitExceeded = ("NEGATIVE_LIMIT_EXCEEDED", 422, "This request goes beyond the allowed negative balance."),
    Overlap = ("OVERLAP", 422, "You already have leave on some of these dates."),
    BackdateNotAllowed = ("BACKDATE_NOT_ALLOWED", 422, "This leave type cannot start that far in the past."),
    NoticeTooShort = ("NOTICE_TOO_SHORT", 422, "This leave type needs more notice."),
    MaxDaysExceeded = ("MAX_DAYS_EXCEEDED", 422, "This request is longer than allowed for this leave type."),
    AttachmentRequired = ("ATTACHMENT_REQUIRED", 422, "A supporting document is required for this request."),
    TypeNotAvailable = ("TYPE_NOT_AVAILABLE", 422, "This leave type is not available to you."),
    TooManyNotify = ("TOO_MANY_NOTIFY", 422, "You can notify at most 20 people."),
    ReasonRequired = ("REASON_REQUIRED", 422, "A reason is required."),
    ReasonTooLong = ("REASON_TOO_LONG", 422, "The reason must be 500 characters or fewer."),
    CompOffDateInvalid = ("COMP_OFF_DATE_INVALID", 422, "Comp-off can only be claimed for a past weekend or mandatory holiday not already claimed."),
    CreditInUse = ("CREDIT_IN_USE", 422, "This comp-off credit is already used or reserved."),
    NoApproverAvailable = ("NO_APPROVER_AVAILABLE", 422, "There is no approver who can decide this request."),
    SelfApproval = ("SELF_APPROVAL", 403, "You cannot decide your own request."),
    NotEligibleApprover = ("NOT_ELIGIBLE_APPROVER", 403, "You are not an approver for this request."),
    AlreadyDecided = ("ALREADY_DECIDED", 409, "This request has already been decided."),
    AlreadyAnonymised = ("ALREADY_ANONYMISED", 409, "This person has already been anonymised."),
    CommentRequired = ("COMMENT_REQUIRED", 422, "A comment is required to reject."),
    NoteRequired = ("NOTE_REQUIRED", 422, "A reason is required for this change."),
    LastAdmin = ("LAST_ADMIN", 422, "The organization must keep at least one active admin."),
    LeaveYearLocked = ("LEAVE_YEAR_LOCKED", 409, "The leave year start can't change once leave data exists."),
    QuotaApplyFromRequired = ("QUOTA_APPLY_FROM_REQUIRED", 422, "Choose whether the new quota applies now or from next leave year."),
    // Organization and domains
    DomainNotRegistered = ("DOMAIN_NOT_REGISTERED", 422, "Your company isn't on Leavo yet. Ask your HR to register."),
    DomainTaken = ("DOMAIN_TAKEN", 409, "This domain is already registered. Contact your admin."),
    PublicDomain = ("PUBLIC_DOMAIN", 422, "Public email domains can't be used."),
    InvalidDomain = ("INVALID_DOMAIN", 422, "That is not a valid domain name."),
    // Access, transport and generic
    Unauthenticated = ("UNAUTHENTICATED", 401, "Please sign in."),
    Forbidden = ("FORBIDDEN", 403, "You don't have access to this."),
    Csrf = ("CSRF", 403, "Request could not be verified."),
    SetupTokenInvalid = ("SETUP_TOKEN_INVALID", 403, "The setup token is not valid."),
    CodeInvalid = ("CODE_INVALID", 422, "That code is not valid or has expired."),
    Locked = ("LOCKED", 429, "Too many wrong attempts. Try again later."),
    RateLimited = ("RATE_LIMITED", 429, "Too many requests. Try again later."),
    NotFound = ("NOT_FOUND", 404, "Not found."),
    IdempotencyMismatch = ("IDEMPOTENCY_MISMATCH", 409, "This request key was already used for a different request."),
    Validation = ("VALIDATION", 422, "Some fields are not valid."),
    FileTooLarge = ("FILE_TOO_LARGE", 413, "The file is larger than 5 MB."),
    FileTypeNotAllowed = ("FILE_TYPE_NOT_ALLOWED", 415, "Only PDF and image files are allowed."),
    ImportHasErrors = ("IMPORT_HAS_ERRORS", 422, "Fix the errors in the file before importing."),
    Internal = ("INTERNAL", 500, "Something went wrong."),
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A domain-level failure: a catalogue code plus optional structured details.
#[derive(Debug, Clone, PartialEq)]
pub struct DomainError {
    pub code: ErrorCode,
    pub details: Option<serde_json::Value>,
}

impl DomainError {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            details: None,
        }
    }
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl From<ErrorCode> for DomainError {
    fn from(code: ErrorCode) -> Self {
        Self::new(code)
    }
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}

impl std::error::Error for DomainError {}
