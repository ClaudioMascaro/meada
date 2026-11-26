package main

import "net/http"

type AnonSvc struct{ cfg *AnonCfg }

// A receiver's field as the head.
func (s *AnonSvc) Get() { http.Get(s.cfg.Kafka.Topics.Orders + "/a") }

// A local as the head.
func AnonLocal() {
	cfg := &AnonCfg{}
	http.Get(cfg.Bus.Topics.Events + "/b")
}

// A local typed by a call's result.
func AnonLoaded() {
	cfg := LoadAnon()
	http.Get(cfg.Kafka.Topics.Orders + "/c")
}

// Two named steps before the anonymous one, from a load with two results.
func AnonNamed() {
	cfg, err := LoadNamed()
	if err != nil {
		return
	}
	http.Get(cfg.Queue.Broker.Topics.Events + "/d")
}
