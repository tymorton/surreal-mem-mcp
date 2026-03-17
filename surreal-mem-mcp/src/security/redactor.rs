use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref OPENAI_PROJ_KEY: Regex = Regex::new(r"sk-proj-[a-zA-Z0-9_\-]{32,}").unwrap();
    static ref OPENAI_KEY: Regex = Regex::new(r"sk-[a-zA-Z0-9]{32,}").unwrap();
    static ref AWS_ACCESS_KEY: Regex = Regex::new(r"(?i)\b(AKIA|A3T|AGPA|AIDA|AROA|AIPA|ANPA|ANVA|ASIA)[A-Z0-9]{16}\b").unwrap();
    static ref AWS_SECRET_KEY: Regex = Regex::new(r#"(?i)\bAWS_?SECRET_?(ACCESS)?_?KEY\s*[=:]\s*['"]?[A-Za-z0-9/+=]{40}['"]?\b"#).unwrap();
    static ref CREDIT_CARD: Regex = Regex::new(r"\b(?:\d{4}[ \-]?){3}\d{4}\b").unwrap(); // stricter block matching
    static ref STRIPE_KEY: Regex = Regex::new(r"(?i)(sk|pk)_(test|live)_[0-9a-zA-Z]{24,}").unwrap();
    static ref GITHUB_PAT: Regex = Regex::new(r"gh[pousr]_[A-Za-z0-9_]{36}").unwrap();
    static ref ANTHROPIC_KEY: Regex = Regex::new(r"sk-ant-api03-[a-zA-Z0-9\-_]{90,}").unwrap();
    static ref SLACK_TOKEN: Regex = Regex::new(r"xox[baprs]-[a-zA-Z0-9\-]+").unwrap();
    static ref GOOGLE_API_KEY: Regex = Regex::new(r"AIza[0-9A-Za-z\-_]{35}").unwrap();
    static ref GOOGLE_OAUTH: Regex = Regex::new(r"ya29\.[0-9A-Za-z\-_]+").unwrap();
}

pub fn redact_sensitive_data(input: &str) -> String {
    let mut redacted = input.to_string();
    redacted = OPENAI_PROJ_KEY.replace_all(&redacted, "<REDACTED_OPENAI_API_KEY>").to_string();
    redacted = OPENAI_KEY.replace_all(&redacted, "<REDACTED_OPENAI_API_KEY>").to_string();
    redacted = AWS_ACCESS_KEY.replace_all(&redacted, "<REDACTED_AWS_ACCESS_KEY>").to_string();
    redacted = AWS_SECRET_KEY.replace_all(&redacted, "<REDACTED_AWS_SECRET_KEY>").to_string();
    redacted = STRIPE_KEY.replace_all(&redacted, "<REDACTED_STRIPE_API_KEY>").to_string();
    redacted = GITHUB_PAT.replace_all(&redacted, "<REDACTED_GITHUB_TOKEN>").to_string();
    redacted = ANTHROPIC_KEY.replace_all(&redacted, "<REDACTED_ANTHROPIC_API_KEY>").to_string();
    redacted = SLACK_TOKEN.replace_all(&redacted, "<REDACTED_SLACK_TOKEN>").to_string();
    redacted = GOOGLE_API_KEY.replace_all(&redacted, "<REDACTED_GOOGLE_API_KEY>").to_string();
    redacted = GOOGLE_OAUTH.replace_all(&redacted, "<REDACTED_GOOGLE_OAUTH_TOKEN>").to_string();
    redacted = CREDIT_CARD.replace_all(&redacted, "<REDACTED_CREDIT_CARD>").to_string();
    
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_sensitive_data() {
        let input = "Here is my secret sk-proj-1234567890abcdef1234567890abcdef1234 and my github ghx_invalid and actual ghp_123456789012345678901234567890123456";
        let output = redact_sensitive_data(input);
        
        assert!(output.contains("<REDACTED_OPENAI_API_KEY>"));
        assert!(output.contains("ghx_invalid")); // Should not redact
        assert!(output.contains("<REDACTED_GITHUB_TOKEN>"));
        assert!(!output.contains("sk-proj-1234567890abcdef"));
        assert!(!output.contains("ghp_123456789012345678901234567890123456"));

        let input_aws = "Set AWS_SECRET_ACCESS_KEY=1234567890123456789012345678901234567890";
        let output_aws = redact_sensitive_data(input_aws);
        assert!(output_aws.contains("<REDACTED_AWS_SECRET_KEY>"));
        
        // Simple credit card
        let input_cc = "My card is 1234-5678-9012-3456 today";
        let output_cc = redact_sensitive_data(input_cc);
        assert!(output_cc.contains("<REDACTED_CREDIT_CARD>"));

        // Extended Providers
        let input_ext = "Google AIzaSyB_1234567890abcdef1234567890abcdef and Anthropic sk-ant-api03-abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHI";
        let output_ext = redact_sensitive_data(input_ext);
        assert!(output_ext.contains("<REDACTED_GOOGLE_API_KEY>"));
        assert!(output_ext.contains("<REDACTED_ANTHROPIC_API_KEY>"));
        assert!(!output_ext.contains("AIzaSyB_"));
        assert!(!output_ext.contains("sk-ant-api03"));
    }
}
