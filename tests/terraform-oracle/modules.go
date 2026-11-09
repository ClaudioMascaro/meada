package main

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/hashicorp/hcl/v2"
	"github.com/hashicorp/hcl/v2/ext/typeexpr"
	"github.com/hashicorp/hcl/v2/hclsyntax"
	"github.com/zclconf/go-cty/cty"
	"github.com/zclconf/go-cty/cty/convert"
)

// A module case is a directory under modules/ holding a root module (*.tf) and, optionally, the
// local module `m/` that the root calls as `module "m" { source = "./m" ... }`. Its `want` file is
// `<must>\t<keys>`: the keys of the `for_each` of the resource `a.q` (the one in `m/` when the
// root has none), sorted and joined by commas, which `go run .` rewrites; `=` means Meada must
// answer them exactly, `~` that it may leave them unresolved. The test in src/iac.rs indexes the
// directory's files.
//
// Terraform's own rules at the boundaries a name crosses are applied here: a variable's declared
// type converts the value it receives, a module argument is evaluated in the caller, and an
// override file's `type` and `default` replace the declared ones.

type variable struct{ typ, def hclsyntax.Expression }

type module struct {
	vars   map[string]*variable
	locals map[string]hclsyntax.Expression
	blocks []*hclsyntax.Block
}

func loadModule(dir string) (*module, error) {
	paths, _ := filepath.Glob(filepath.Join(dir, "*.tf"))
	isOverride := func(p string) bool {
		b := filepath.Base(p)
		return b == "override.tf" || strings.HasSuffix(b, "_override.tf")
	}
	// Override files merge into the others, which are all read first.
	sort.SliceStable(paths, func(i, j int) bool { return !isOverride(paths[i]) && isOverride(paths[j]) })
	m := &module{vars: map[string]*variable{}, locals: map[string]hclsyntax.Expression{}}
	for _, p := range paths {
		src, err := os.ReadFile(p)
		if err != nil {
			return nil, err
		}
		file, diags := hclsyntax.ParseConfig(src, p, hcl.InitialPos)
		if diags.HasErrors() {
			return nil, diags
		}
		for _, b := range file.Body.(*hclsyntax.Body).Blocks {
			switch b.Type {
			case "variable":
				v := m.vars[b.Labels[0]]
				if v == nil {
					v = &variable{}
					m.vars[b.Labels[0]] = v
				}
				if a, ok := b.Body.Attributes["type"]; ok {
					v.typ = a.Expr
				}
				if a, ok := b.Body.Attributes["default"]; ok {
					v.def = a.Expr
				}
			case "locals":
				for name, a := range b.Body.Attributes {
					m.locals[name] = a.Expr
				}
			default:
				m.blocks = append(m.blocks, b)
			}
		}
	}
	return m, nil
}

// values are the module's variables given `given` (else their defaults), converted to their
// declared types.
func (m *module) values(given map[string]cty.Value) (map[string]cty.Value, error) {
	out := map[string]cty.Value{}
	for name, v := range m.vars {
		val, ok := given[name]
		if !ok && v.def != nil {
			var diags hcl.Diagnostics
			val, diags = v.def.Value(&hcl.EvalContext{Functions: functions})
			if diags.HasErrors() {
				return nil, diags
			}
			ok = true
		}
		if !ok {
			continue
		}
		if v.typ != nil {
			ty, diags := typeexpr.TypeConstraint(v.typ)
			if diags.HasErrors() {
				return nil, diags
			}
			var err error
			if val, err = convert.Convert(val, ty); err != nil {
				return nil, err
			}
		}
		out[name] = val
	}
	return out, nil
}

// context evaluates the locals, one pass per local so a chain of them settles.
func (m *module) context(vars map[string]cty.Value) *hcl.EvalContext {
	locals := map[string]cty.Value{}
	ctx := func() *hcl.EvalContext {
		return &hcl.EvalContext{Functions: functions, Variables: map[string]cty.Value{
			"var": cty.ObjectVal(vars), "local": cty.ObjectVal(locals),
		}}
	}
	for range m.locals {
		c := ctx()
		for name, e := range m.locals {
			if v, diags := e.Value(c); !diags.HasErrors() {
				locals[name] = v
			}
		}
	}
	return ctx()
}

func (m *module) block(kind, label string) *hclsyntax.Block {
	for _, b := range m.blocks {
		if b.Type == kind && b.Labels[len(b.Labels)-1] == label {
			return b
		}
	}
	return nil
}

func forEachKeys(m *module, ctx *hcl.EvalContext) (string, bool) {
	b := m.block("resource", "q")
	if b == nil {
		return "", false
	}
	v, diags := b.Body.Attributes["for_each"].Expr.Value(ctx)
	if diags.HasErrors() {
		return "!" + diags[0].Summary, true
	}
	var keys []string
	switch ty := v.Type(); {
	case ty.IsSetType():
		for it := v.ElementIterator(); it.Next(); {
			_, e := it.Element()
			keys = append(keys, e.AsString())
		}
	case ty.IsObjectType() || ty.IsMapType():
		for k := range v.AsValueMap() {
			keys = append(keys, k)
		}
	default:
		return "!for_each is no set or map", true
	}
	sort.Strings(keys)
	return strings.Join(keys, ","), true
}

// moduleKeys are the keys of the case in `dir`, or `!` and the first error.
func moduleKeys(dir string) string {
	root, err := loadModule(dir)
	if err != nil {
		return "!" + err.Error()
	}
	vars, err := root.values(nil)
	if err != nil {
		return "!" + err.Error()
	}
	ctx := root.context(vars)
	if keys, ok := forEachKeys(root, ctx); ok {
		return keys
	}
	call := root.block("module", "m")
	child, err := loadModule(filepath.Join(dir, "m"))
	if call == nil || err != nil {
		return "!no resource a.q"
	}
	given := map[string]cty.Value{}
	for name, a := range call.Body.Attributes {
		if name == "source" {
			continue
		}
		v, diags := a.Expr.Value(ctx)
		if diags.HasErrors() {
			return "!" + diags[0].Summary
		}
		given[name] = v
	}
	vars, err = child.values(given)
	if err != nil {
		return "!" + err.Error()
	}
	keys, _ := forEachKeys(child, child.context(vars))
	return keys
}

// moduleCases rewrites the `want` file of each case under dir/modules; it returns how many it changed.
func moduleCases(dir string) (changed, total int, err error) {
	wants, _ := filepath.Glob(filepath.Join(dir, "modules", "*", "want"))
	for _, want := range wants {
		text, err := os.ReadFile(want)
		if err != nil {
			return 0, 0, err
		}
		cols := strings.Split(strings.TrimSuffix(string(text), "\n"), "\t")
		if len(cols) != 2 || (cols[0] != "=" && cols[0] != "~") {
			return 0, 0, fmt.Errorf("%s: want `=` or `~` and the keys, tab-separated", want)
		}
		total++
		if got := moduleKeys(filepath.Dir(want)); got != cols[1] {
			changed++
			line := cols[0] + "\t" + got + "\n"
			if err := os.WriteFile(want, []byte(line), 0o644); err != nil {
				return 0, 0, err
			}
		}
	}
	return changed, total, nil
}
