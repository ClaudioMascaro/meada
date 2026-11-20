package evidence

import "net/http"

func plain(u string) { _, _ = http.Get(u) }

func PlainA() { plain("http://svc/plain-a") }

func get(n int, p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18, p19, p20, p21, p22, p23 string) {
	_, _ = http.Get(p0)
	if n > 0 {
		get(n-1, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18, p19, p20, p21, p22, p23, p0)
		get(n-1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p15, p16, p17, p18, p19, p20, p21, p22, p23, p0, p1)
	}
}

func UseGet() { get(1, "http://svc/g0", "http://svc/g1", "http://svc/g2", "http://svc/g3", "http://svc/g4", "http://svc/g5", "http://svc/g6", "http://svc/g7", "http://svc/g8", "http://svc/g9", "http://svc/g10", "http://svc/g11", "http://svc/g12", "http://svc/g13", "http://svc/g14", "http://svc/g15", "http://svc/g16", "http://svc/g17", "http://svc/g18", "http://svc/g19", "http://svc/g20", "http://svc/g21", "http://svc/g22", "http://svc/g23") }

func PlainB() { plain("http://svc/plain-b") }
