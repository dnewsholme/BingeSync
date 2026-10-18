use url::Url;

/// Validates that a user-supplied external service URL is safe to make HTTP requests to.
/// Blocks non-HTTP/HTTPS protocols and dangerous cloud metadata endpoints.
pub fn validate_integration_url(raw_url: &str) -> Result<Url, String> {
    let parsed = Url::parse(raw_url.trim()).map_err(|e| format!("Invalid URL: {}", e))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("Unsupported scheme '{}'. Only 'http' and 'https' are allowed", scheme));
    }

    match parsed.host() {
        Some(url::Host::Ipv4(ipv4)) => {
            let octets = ipv4.octets();
            // 169.254.0.0/16 - Link-local & cloud metadata
            if octets[0] == 169 && octets[1] == 254 {
                return Err("Access to link-local and cloud metadata addresses (169.254.x.x) is forbidden".to_string());
            }
            // 100.100.100.200 (Alibaba Cloud metadata)
            if octets[0] == 100 && octets[1] == 100 && octets[2] == 100 && octets[3] == 200 {
                return Err("Access to cloud metadata addresses is forbidden".to_string());
            }
            // 0.0.0.0
            if ipv4.is_unspecified() {
                return Err("Access to 0.0.0.0 is forbidden".to_string());
            }
        }
        Some(url::Host::Ipv6(ipv6)) => {
            let segments = ipv6.segments();
            // fe80::/10 - Link-local
            if (segments[0] & 0xffc0) == 0xfe80 {
                return Err("Access to link-local IPv6 addresses is forbidden".to_string());
            }
            // fd00:ec2::254 - AWS IMDSv2 IPv6
            if segments[0] == 0xfd00 && segments[1] == 0x0ec2 {
                return Err("Access to cloud metadata addresses is forbidden".to_string());
            }
            if ipv6.is_unspecified() {
                return Err("Access to unspecified IPv6 address is forbidden".to_string());
            }
        }
        Some(url::Host::Domain(domain)) => {
            let host_lower = domain.to_ascii_lowercase();
            // Block common cloud metadata hostnames
            if host_lower == "metadata.google.internal"
                || host_lower == "metadata"
                || host_lower == "instance-data"
            {
                return Err("Access to cloud metadata endpoints is forbidden".to_string());
            }
        }
        None => return Err("URL must include a valid host".to_string()),
    }

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_integration_urls() {
        assert!(validate_integration_url("http://localhost:8989").is_ok());
        assert!(validate_integration_url("http://192.168.1.100:7878").is_ok());
        assert!(validate_integration_url("https://radarr.example.com/api/v3").is_ok());
        assert!(validate_integration_url("http://sonarr:8989").is_ok());
    }

    #[test]
    fn test_invalid_scheme() {
        assert!(validate_integration_url("file:///etc/passwd").is_err());
        assert!(validate_integration_url("gopher://127.0.0.1").is_err());
        assert!(validate_integration_url("ftp://server.local").is_err());
    }

    #[test]
    fn test_metadata_ssrf_blocked() {
        assert!(validate_integration_url("http://169.254.169.254/latest/meta-data").is_err());
        assert!(validate_integration_url("http://metadata.google.internal/computeMetadata/v1").is_err());
        assert!(validate_integration_url("http://100.100.100.200/").is_err());
        assert!(validate_integration_url("http://[fd00:ec2::254]/").is_err());
    }
}
