//go:build linux && (amd64 || arm64) && !cgo

package store

func onLinux() {
	helper()
}
