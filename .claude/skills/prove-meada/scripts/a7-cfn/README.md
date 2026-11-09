# a7-cfn

The CloudFormation and SAM probe of prove-meada (#653), each shape read by `cfn_oracle.py` on its
own (PyYAML with a constructor for every short-form tag, and `yaml.compose` for the lines).

- `sam/template.yaml`: a SAM application (function, API, queue, table) with parameters
- `stacks/messaging.yaml`: parameters (a `NoEcho` one, a list, one with no default), `Ref`,
  `Fn::GetAtt` in both forms, `Fn::Sub` with and without a map and with `${!Literal}`, `Fn::Join`
  in short and long form, `!ImportValue`, a pseudo parameter, `DependsOn` as a list, a resource
  with no name and a custom resource
- `stacks/events.json`: a JSON template in the long forms
- `stacks/legacy.yml`: a second `JobsQueue`, so the address fits two nodes
- `nested/not-a-template.yaml`: `Resources` with no version or transform: no node
- `zz_code.py`: a Python class `OrdersQueue`, named like a node: its `context` and `trace` must
  stay the base's
- `stacks/messaging.yaml` also holds a `NoEcho: yes` parameter, an SSM parameter value, a resource
  whose `Properties` come before its `Type` and has a property spelled like its logical id, and a
  `Fn::GetAtt` with a computed attribute
