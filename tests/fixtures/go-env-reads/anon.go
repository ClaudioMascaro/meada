package main

// Tagged fields inside anonymous structs, read from another file: after the named type, and
// after two named types reached through fields.
type AnonCfg struct {
	Kafka struct {
		Topics struct {
			Orders string `env:"ANON_URL"`
		}
	}
	Bus struct {
		Topics struct {
			Events string `envconfig:"ANON_BUS_URL"`
		}
	}
	Queue AnonQueue
}

type AnonQueue struct{ Broker AnonBroker }

type AnonBroker struct {
	Topics struct {
		Events string `envconfig:"ANON_BROKER_URL"`
	}
}

func LoadAnon() *AnonCfg { return &AnonCfg{} }

func LoadNamed() (*AnonCfg, error) { return &AnonCfg{}, nil }
