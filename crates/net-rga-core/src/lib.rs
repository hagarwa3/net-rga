pub fn workspace_bootstrapped() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::workspace_bootstrapped;

    #[test]
    fn workspace_bootstrap_flag_is_true() {
        assert!(workspace_bootstrapped());
    }
}
