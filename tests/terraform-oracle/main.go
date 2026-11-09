// Command terraform-oracle computes, with HashiCorp's HCL and cty libraries, the name Terraform
// gives each case of cases.tsv, and rewrites the file's last column with it.
//
// Usage (from this directory): go run . [cases.tsv]
//
// A case is `<must>\t<expression>\t<terraform>`. The test in src/iac.rs indexes
// `resource "a" "rN" { name = "q-${<expression>}" }` for each one: an exact name must equal
// <terraform>, an error (`!<summary>`) allows none, and a row marked `=` must be exact.
//
// The functions are the ones Terraform takes as they are from cty's stdlib, plus the to*
// conversions, which Terraform makes with cty's convert package. modules.go does the same for
// the cases of modules/. Terraform's own functions
// (length, lookup, coalesce, replace, ...) are absent, so a case calling one reads as an error.
package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/hashicorp/hcl/v2"
	"github.com/hashicorp/hcl/v2/hclsyntax"
	"github.com/zclconf/go-cty/cty"
	"github.com/zclconf/go-cty/cty/convert"
	"github.com/zclconf/go-cty/cty/function"
	"github.com/zclconf/go-cty/cty/function/stdlib"
)

var functions = map[string]function.Function{
	"chomp":      stdlib.ChompFunc,
	"concat":     stdlib.ConcatFunc,
	"element":    stdlib.ElementFunc,
	"format":     stdlib.FormatFunc,
	"formatlist": stdlib.FormatListFunc,
	"join":       stdlib.JoinFunc,
	"jsonencode": stdlib.JSONEncodeFunc,
	"keys":       stdlib.KeysFunc,
	"lower":      stdlib.LowerFunc,
	"merge":      stdlib.MergeFunc,
	"split":      stdlib.SplitFunc,
	"substr":     stdlib.SubstrFunc,
	"title":      stdlib.TitleFunc,
	"trim":       stdlib.TrimFunc,
	"trimprefix": stdlib.TrimPrefixFunc,
	"trimspace":  stdlib.TrimSpaceFunc,
	"trimsuffix": stdlib.TrimSuffixFunc,
	"upper":      stdlib.UpperFunc,
	"values":     stdlib.ValuesFunc,
	"tostring":   to(cty.String),
	"tonumber":   to(cty.Number),
	"tobool":     to(cty.Bool),
	"tolist":     to(cty.List(cty.DynamicPseudoType)),
	"toset":      to(cty.Set(cty.DynamicPseudoType)),
	"tomap":      to(cty.Map(cty.DynamicPseudoType)),
}

func to(want cty.Type) function.Function {
	return function.New(&function.Spec{
		Params: []function.Parameter{{Name: "v", Type: cty.DynamicPseudoType, AllowNull: true, AllowDynamicType: true}},
		Type: func(args []cty.Value) (cty.Type, error) {
			v, err := convert.Convert(args[0], want)
			return v.Type(), err
		},
		Impl: func(args []cty.Value, ty cty.Type) (cty.Value, error) {
			return convert.Convert(args[0], ty)
		},
	})
}

// name is what Terraform makes of `name = "q-${expression}"`, or `!` and the first error.
func name(expression string) string {
	src := fmt.Sprintf("name = \"q-${%s}\"\n", expression)
	file, diags := hclsyntax.ParseConfig([]byte(src), "case.tf", hcl.InitialPos)
	if diags.HasErrors() {
		return "!" + diags[0].Summary
	}
	attrs, diags := file.Body.JustAttributes()
	if diags.HasErrors() {
		return "!" + diags[0].Summary
	}
	v, diags := attrs["name"].Expr.Value(&hcl.EvalContext{Functions: functions})
	if diags.HasErrors() {
		return "!" + diags[0].Summary
	}
	if v.IsNull() || !v.IsKnown() || !v.Type().Equals(cty.String) {
		return "!not a known string"
	}
	return v.AsString()
}

func main() {
	path := "cases.tsv"
	if len(os.Args) > 1 {
		path = os.Args[1]
	}
	text, err := os.ReadFile(path)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	lines := strings.Split(strings.TrimSuffix(string(text), "\n"), "\n")
	changed := 0
	for i, line := range lines {
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		cols := strings.Split(line, "\t")
		if len(cols) != 3 || (cols[0] != "=" && cols[0] != "~") {
			fmt.Fprintf(os.Stderr, "%s:%d: want `=` or `~`, an expression and a name, tab-separated\n", path, i+1)
			os.Exit(1)
		}
		got := name(cols[1])
		if strings.ContainsAny(got, "\t\n") {
			fmt.Fprintf(os.Stderr, "%s:%d: the name holds a tab or newline: %q\n", path, i+1, got)
			os.Exit(1)
		}
		if got != cols[2] {
			changed++
			lines[i] = strings.Join([]string{cols[0], cols[1], got}, "\t")
		}
	}
	if err := os.WriteFile(path, []byte(strings.Join(lines, "\n")+"\n"), 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("%d of %d lines rewritten\n", changed, len(lines))
	n, total, err := moduleCases(filepath.Dir(path))
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("%d of %d module cases rewritten\n", n, total)
}
