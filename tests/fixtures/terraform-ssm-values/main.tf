resource "aws_ssm_parameter" "before" {
  value = "ssm-hcl-before-canary"
  type  = "SecureString"
}

resource "aws_ssm_parameter" "after" {
  type  = "SecureString"
  value = "ssm-hcl-after-canary"
}

resource "aws_ssm_parameter" "plain" {
  type  = "String"
  value = "ssm-hcl-public-value"
}
