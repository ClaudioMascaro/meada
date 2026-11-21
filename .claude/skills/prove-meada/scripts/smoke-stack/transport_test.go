package ordersclient

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"testing"
)

type Recorder func(*http.Request) (*http.Response, error)

func (record Recorder) RoundTrip(request *http.Request) (*http.Response, error) {
	return record(request)
}

func TestTransport(t *testing.T) {
	saved := http.DefaultClient
	t.Cleanup(func() { http.DefaultClient = saved })
	for _, probe := range []struct {
		name string
		send func() (*http.Response, error)
	}{{"SendShared", SendShared}, {"SendConfigured", SendConfigured}} {
		http.DefaultClient = &http.Client{Transport: Recorder(func(request *http.Request) (*http.Response, error) {
			point, err := json.Marshal(map[string]string{
				"caller": probe.name, "method": request.Method, "url": request.URL.String(),
			})
			if err != nil {
				t.Fatal(err)
			}
			fmt.Println("PROBE " + string(point))
			return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(""))}, nil
		})}
		response, err := probe.send()
		if err != nil {
			t.Fatal(err)
		}
		response.Body.Close()
	}
}
