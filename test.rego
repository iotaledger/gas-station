package test

import rego.v1

default allow := false

allow if {
	input.method == "GET"
}
