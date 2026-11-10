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
// answer them exactly, `~` that it may leave them unresolved. `!` and a reason stand for keys
// Terraform refuses; no key at all is an empty list. The test in src/iac/tests/oracle.rs indexes
// the directory's files.
//
// Modelled, with Terraform's rules: a variable block's default is converted to the type that
// block declares when it is decoded; an override file's variable block replaces the declared
// `type` and `default`, and the default is converted again to the final type; a module argument
// is evaluated in the caller and converted to the variable's declared type; locals, variables
// (defaults and arguments only: no var files) and the functions of main.go. Anything else makes
// `go run .` stop with an error naming the case, so a case outside the model cannot get a
// `want`: an override file's resource, module or undeclared variable block, a local that does not
// evaluate, a module argument the module does not declare, a variable without a value, a second
// module call or a resource other than `a.q`.

// variable is a declared variable: its type (any without a `type`) and default (cty.NilVal
// without one), as Terraform keeps them.
type variable struct {
	ty  cty.Type
	def cty.Value
}

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
		override := isOverride(p)
		src, err := os.ReadFile(p)
		if err != nil {
			return nil, err
		}
		file, diags := hclsyntax.ParseConfig(src, p, hcl.InitialPos)
		if diags.HasErrors() {
			return nil, diags
		}
		for _, b := range file.Body.(*hclsyntax.Body).Blocks {
			switch {
			case b.Type == "variable":
				if err := m.declare(b, override); err != nil {
					return nil, fmt.Errorf("%s: %w", p, err)
				}
			case b.Type == "locals":
				for name, a := range b.Body.Attributes {
					m.locals[name] = a.Expr
				}
			case override:
				return nil, fmt.Errorf("%s: an override %s block is not modelled", p, b.Type)
			case b.Type == "resource" && len(b.Labels) == 2 && b.Labels[0] == "a" && b.Labels[1] == "q",
				b.Type == "module" && len(b.Labels) == 1 && b.Labels[0] == "m":
				if m.block(b.Type, b.Labels[len(b.Labels)-1]) != nil {
					return nil, fmt.Errorf("%s: a second %s block %v", p, b.Type, b.Labels)
				}
				m.blocks = append(m.blocks, b)
			default:
				return nil, fmt.Errorf("%s: a %s block %v is not modelled", p, b.Type, b.Labels)
			}
		}
	}
	return m, nil
}

// declare reads a variable block as Terraform does: the default is converted to the type this
// block declares, and an override's `type` and `default` replace the declared ones, the default
// then converted again to the final type.
func (m *module) declare(b *hclsyntax.Block, override bool) error {
	name := b.Labels[0]
	block := &variable{ty: cty.DynamicPseudoType, def: cty.NilVal}
	hasType := false
	if a, ok := b.Body.Attributes["type"]; ok {
		ty, diags := typeexpr.TypeConstraint(a.Expr)
		if diags.HasErrors() {
			return diags
		}
		block.ty, hasType = ty, true
	}
	if a, ok := b.Body.Attributes["default"]; ok {
		val, diags := a.Expr.Value(&hcl.EvalContext{Functions: functions})
		if diags.HasErrors() {
			return diags
		}
		if hasType {
			var err error
			if val, err = convert.Convert(val, block.ty); err != nil {
				return fmt.Errorf("variable %q: its default does not convert: %w", name, err)
			}
		}
		block.def = val
	}
	v := m.vars[name]
	switch {
	case v == nil && override:
		return fmt.Errorf("an override of the undeclared variable %q", name)
	case v != nil && !override:
		return fmt.Errorf("variable %q is declared twice", name)
	case v == nil:
		m.vars[name] = block
		return nil
	}
	if block.def != cty.NilVal {
		v.def = block.def
	}
	if hasType {
		v.ty = block.ty
	}
	if v.def != cty.NilVal {
		def, err := convert.Convert(v.def, v.ty)
		if err != nil {
			return fmt.Errorf("variable %q: its default does not convert to the overriding type: %w", name, err)
		}
		v.def = def
	}
	return nil
}

// values are the module's variables: what `given` says, else the default, converted to the
// declared type. A variable without a value, or an argument for none, ends the case.
func (m *module) values(given map[string]cty.Value) (map[string]cty.Value, error) {
	for name := range given {
		if m.vars[name] == nil {
			return nil, fmt.Errorf("an argument for the undeclared variable %q", name)
		}
	}
	out := map[string]cty.Value{}
	for name, v := range m.vars {
		val, ok := given[name]
		if !ok && v.def != cty.NilVal {
			val, ok = v.def, true
		}
		if !ok {
			return nil, fmt.Errorf("variable %q has no value", name)
		}
		val, err := convert.Convert(val, v.ty)
		if err != nil {
			return nil, fmt.Errorf("variable %q: %w", name, err)
		}
		out[name] = val
	}
	return out, nil
}

// context evaluates the locals, one pass per local so a chain of them settles; one that still
// does not evaluate ends the case.
func (m *module) context(vars map[string]cty.Value) (*hcl.EvalContext, error) {
	locals := map[string]cty.Value{}
	ctx := func() *hcl.EvalContext {
		return &hcl.EvalContext{Functions: functions, Variables: map[string]cty.Value{
			"var": cty.ObjectVal(vars), "local": cty.ObjectVal(locals),
		}}
	}
	failed := map[string]string{}
	for range m.locals {
		c := ctx()
		for name, e := range m.locals {
			if _, done := locals[name]; done {
				continue
			}
			if v, diags := e.Value(c); diags.HasErrors() {
				failed[name] = diags[0].Summary
			} else {
				locals[name] = v
				delete(failed, name)
			}
		}
	}
	for name, why := range failed {
		return nil, fmt.Errorf("local %q does not evaluate: %s", name, why)
	}
	return ctx(), nil
}

func (m *module) block(kind, label string) *hclsyntax.Block {
	for _, b := range m.blocks {
		if b.Type == kind && b.Labels[len(b.Labels)-1] == label {
			return b
		}
	}
	return nil
}

// forEachKeys are the instance keys of `a.q` in `m` (ok is false without that resource), or `!`
// and the reason Terraform refuses its `for_each`.
func forEachKeys(m *module, ctx *hcl.EvalContext) (keys string, ok bool) {
	b := m.block("resource", "q")
	if b == nil {
		return "", false
	}
	a, has := b.Body.Attributes["for_each"]
	if !has {
		return "!no for_each", true
	}
	v, diags := a.Expr.Value(ctx)
	if diags.HasErrors() {
		return "!" + diags[0].Summary, true
	}
	if v.IsNull() || !v.IsWhollyKnown() {
		return "!for_each is null or unknown", true
	}
	var list []string
	switch ty := v.Type(); {
	case ty.IsSetType() && ty.ElementType() == cty.String:
		for it := v.ElementIterator(); it.Next(); {
			_, e := it.Element()
			list = append(list, e.AsString())
		}
	case ty.IsObjectType() || ty.IsMapType():
		for k := range v.AsValueMap() {
			list = append(list, k)
		}
	default:
		return "!for_each is no map or set of strings", true
	}
	sort.Strings(list)
	return strings.Join(list, ","), true
}

// moduleKeys are the keys of the case in `dir`, or `!` and a reason.
func moduleKeys(dir string) (string, error) {
	root, err := loadModule(dir)
	if err != nil {
		return "", err
	}
	vars, err := root.values(nil)
	if err != nil {
		return "", err
	}
	ctx, err := root.context(vars)
	if err != nil {
		return "", err
	}
	if keys, ok := forEachKeys(root, ctx); ok {
		return keys, nil
	}
	call := root.block("module", "m")
	if call == nil {
		return "", fmt.Errorf("no resource a.q and no module m")
	}
	child, err := loadModule(filepath.Join(dir, "m"))
	if err != nil {
		return "", err
	}
	given := map[string]cty.Value{}
	for name, a := range call.Body.Attributes {
		if name == "source" {
			continue
		}
		v, diags := a.Expr.Value(ctx)
		if diags.HasErrors() {
			return "", fmt.Errorf("argument %q: %s", name, diags[0].Summary)
		}
		given[name] = v
	}
	vars, err = child.values(given)
	if err != nil {
		return "", err
	}
	cctx, err := child.context(vars)
	if err != nil {
		return "", err
	}
	keys, ok := forEachKeys(child, cctx)
	if !ok {
		return "", fmt.Errorf("no resource a.q in m")
	}
	return keys, nil
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
		got, err := moduleKeys(filepath.Dir(want))
		if err != nil {
			return 0, 0, fmt.Errorf("%s: %w", filepath.Dir(want), err)
		}
		if got != cols[1] {
			changed++
			line := cols[0] + "\t" + got + "\n"
			if err := os.WriteFile(want, []byte(line), 0o644); err != nil {
				return 0, 0, err
			}
		}
	}
	return changed, total, nil
}
