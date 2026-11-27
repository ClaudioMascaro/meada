package svc

// RunRequest is the body POST /run reads.
type RunRequest struct {
	Name string `json:"name"`
}
