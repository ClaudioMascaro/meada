module example.com/acme/shop

go 1.22

require example.com/acme/billing v0.0.0

// Two directories declare the module: the replace picks one.
replace example.com/acme/billing => ./forks/billing
