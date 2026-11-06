# #683/#684: start with the complete trusted base export, then apply only the specified
# resolutions. The three inline controls supply existing link/reason behavior. Never read
# the head export here: unexpected fields, locations and process metadata must survive comparison.
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
   elif $consumer.via == null and $consumer.contract == "http::GET::/orders"
   then links($template[0]; "ordersTpl")
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
| .consumers |= sort_by(.site)
| .links |= sort_by(.consumer, .provider, .contract)
| del(.commit, .dirty)
