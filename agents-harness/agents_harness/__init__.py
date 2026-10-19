"""The agents' harness (PL-6.1): runs one job inside its sandbox.

`python -m agents_harness` loads `/job/in`, renders the system prompt from the
definition and the skills, runs the agent loop on the OpenAI Agents SDK with
exactly the tools the job's capabilities grant, enforces the budget and writes
`/job/out` (contracts section 3).
"""

__version__ = "0.1.0"
