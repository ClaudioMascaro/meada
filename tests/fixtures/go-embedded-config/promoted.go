package orders

func (svc *Service) Promoted() { consume(svc.cfg.Topics.Shipped) }
