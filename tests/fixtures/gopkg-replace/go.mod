module example.com/gopkg/app

go 1.22

require example.com/orig/lib v1.0.0

replace example.com/orig/lib => example.com/fork/lib v1.0.0-fork.1
