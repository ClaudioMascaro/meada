# Independent native-HCL scalar source. Comments emit no values.
locals {
  proof = {
    empty = ""
    integer = 17
    decimal = 12.50
    exponent = 1.25e+03
    truth = true
    falsity = false
    nothing = null
    escaped = "line\nnext\t\"\\tail"
    unicode = "caf\u00e9"
    refresh = "maples"
    ranked_exact = "MossNeedle"
    ranked_prefix = "mossneedle-top"
    ranked_substring = "rim-mossneedle-tail"
    scan_exact = "qz"
    scan_prefix = "qz-leaf"
    scan_substring = "a-qz-z"
    items = [
      "cypress",
      5,
      false,
      null,
      { "a.b" = "cedar" }
    ]
    object = {
      plain = "ash"
      "dash-key" = "elm"
      "region/east" = "fir"
      "quote\"back\\slash" = "hazel"
      "snow☃" = "holly"
    }
    empty_list = []
    empty_object = {}
    reference = var.proof
    call = upper("seed")
    arithmetic = 2 + 3
    unary = -7
    conditional = var.proof ? "yes" : "no"
    comprehension = [for item in ["x", "y"] : upper(item)]
    dynamic_object = { (var.proof) = "dynamic", literal = "kept-in-expression" }
    multiline = (
      2 +
      3
    )
  }
}
