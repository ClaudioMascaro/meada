# #683/#684: start with the complete trusted base export, then apply only the specified
# resolutions, and #973's Go `type` rows in g9. The inline controls supply existing link/reason
# behavior. Never read the head export here: unexpected fields, locations and process metadata
# must survive comparison. Communities stay the base's; where the calls change they are checked
# by `a8-communities.jq` instead (`compare_exports`).
def function_name: split("::")[2];
def consumers($doc; $fn): [$doc.consumers[] | select((.site | function_name) == $fn)];
def links($doc; $fn): [$doc.links[] | select((.consumer | function_name) == $fn)];
def value($rows; $field; $old):
  [$rows[] | select(.[2] == $field) | .[3]] as $values
  | if ($values | length) == 0 then $old
    elif $values[-1] == "null" then null else $values[-1] end;
def expected_consumer($doc; $rows; $fn):
  consumers($doc; $fn) as $old
  | if value($rows; "contract"; null) == "no consumer" then empty
    else
      (if ($old | length) == 1 then $old[0]
       elif ($old | length) == 0 and ($fn == "ordersJoinParam" or $fn == "ordersJoinParamBase") then
         {contract: null, site: ($doc.repository + "::src/client.ts::" + $fn + "::"
           + (if $fn == "ordersJoinParam" then "joinGet#0" else "joinFrom#0" end)), via: null}
       else error("expected one source call for " + $fn) end) as $consumer
      | $consumer
      | .contract = value($rows; "contract"; .contract)
      | .via = value($rows; "via";
          if (["ordersJoinSlash", "ordersJoinNoSlash", "ordersJoinParam", "ordersJoinParamBase"] | index($fn)) != null
          then "host:api.example.com"
          elif $fn == "listClusters" then $proxy[0].consumers[0].via
          else .via end)
    end;
def expected_links($consumer):
  (if $consumer.via == "env:API_URL" then links($environment[0]; "OrdersInline")
   elif $consumer.via == $proxy[0].consumers[0].via then links($proxy[0]; "listClusters")
   # A hostless `GET /orders`: both peers serve it (`gen_peers`), so one ambiguous link to each. The
   # rows are written out: the template control `ordersTpl` that gave them until #845 is an open item
   # now, and no other hostless call of g8 links.
   elif $consumer.via == null and $consumer.contract == "http::GET::/orders"
   then [("api", "other") | ("example.com/acme/" + .) as $peer
         | {level: "ambiguous", provider: ($peer + "::main.go::orders"),
            reason: ("2 providers fit; host unresolved; served by " + $peer)}]
   else [] end)
  | map(.consumer = $consumer.site | .contract = $consumer.contract);
def terminal_steps($doc; $consumer; $links):
  if $consumer == null then []
  elif ($links | length) == 1 then
    [$links[0] | {depth: 1, repository: (.provider | split("::")[0]), level,
      via: "link", node: .provider} + (if has("reason") then {reason} else {} end)]
  elif ($links | length) > 1 then
    [{depth: 1, repository: $doc.repository, level: "ambiguous", via: "call",
      node: $consumer.site, mark: "ambiguous", candidates: ([$links[].provider] | sort)}]
  else
    [{depth: 1, repository: $doc.repository, level: null, via: "call", node: $consumer.site,
      mark: (if $consumer.contract == null then "unresolved" else "no-provider" end)}
      + (if $consumer.contract == null then {} else {contract: $consumer.contract} end)]
  end;

# #973: "calls: a `type` row from each function, method, type or variable to each type it names",
# exact, once per pair. g9's Go file (`gen_g9`) names repository types only in these definitions;
# the row's span is the type's first mention in the definition (the spec names no position). Read
# from the generated source, never from the head export.
def g9_type_uses($repo):
  [["OrdersInline", "func OrdersInline()", "Config"],
   ["New", "func New(", "Client"],
   ["FromTag", "func FromTag()", "Client"],
   ["FromTag", "func FromTag()", "Config"],
   ["FromGetenv", "func FromGetenv()", "Client"],
   ["Client.send", "func (c *Client) send(", "Client"],
   ["Client.OrdersDirect", "func (c *Client) OrdersDirect(", "Client"],
   ["Client.Orders", "func (c *Client) Orders(", "Client"]][]
  | . as [$caller, $header, $type]
  | ($gosrc | indices($header)) as $at
  | if ($at | length) != 1 then error("g9 source: expected one " + $header) else $at[0] end
  | (. + ($gosrc[.:] | index($type))) as $start
  | ($repo + "::gocli/client.go::") as $file
  | {caller: ($file + $caller), id: ($file + $caller + "::(type)" + $type + "#0"), kind: "type",
     level: "exact", span: {end: ($start + ($type | length)), path: "gocli/client.go", start: $start},
     target: ($file + $type)};

. as $base
| ($cases | split("\n") | map(select(length > 0 and (startswith("#") | not)) | split("\t"))
    | map(select(.[0] == $workspace and .[4] == "landed"))) as $rows
| ([$rows[][1]] | unique) as $functions
| reduce $functions[] as $fn (. ;
    [$rows[] | select(.[1] == $fn)] as $selected
    | [expected_consumer($base; $selected; $fn)] as $new
    | .consumers = ([.consumers[] | select((.site | function_name) != $fn)] + $new)
    | .links = ([.links[] | select((.consumer | function_name) != $fn)]
        + [$new[] | expected_links(.)[]]))
| . as $expected
| .processes |= map(
    (.entry_point | function_name) as $fn
    | if ($functions | index($fn)) == null then . else
        consumers($expected; $fn) as $consumer
        | links($expected; $fn) as $links
        | [.steps[] | select(.via == "link" or has("mark"))] as $old_terminal
        | if ($old_terminal | length) != 1 then error("expected one terminal call for " + $fn) else . end
        | .steps = ([.steps[] | select(.via != "link" and (has("mark") | not))]
            + terminal_steps($base; $consumer[0]; $links))
      end)
# A base that already writes them (1.6.0 and later) keeps its calls, so its communities compare whole.
| ([.calls[].id] | INDEX(.)) as $have
| if $workspace == "g9"
  then .calls = ((.calls + [g9_type_uses($base.repository) | select($have[.id] == null)]) | sort_by(.id))
  else . end
| .consumers |= sort_by(.site)
| .links |= sort_by(.consumer, .provider, .contract)
| del(.commit, .dirty)
