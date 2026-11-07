locals {
  proof = {
    literal = <<PLAIN
root-line
  kept-spaces
PLAIN
    indented = <<-INDENT
      first-thread
        second-thread
      third-thread
    INDENT
    escaped_markers = "$${proof} %%{ proof }"
    interpolation = "prefix-${var.proof}-suffix"
    sole_interpolation = "${var.proof}"
    directive = "%{ if var.proof }yes%{ else }no%{ endif }"
    literal_heredoc_backslash = <<BACKSLASH
\n is not a heredoc newline escape
BACKSLASH
  }
}
