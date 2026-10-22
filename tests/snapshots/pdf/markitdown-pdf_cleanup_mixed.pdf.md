ZAVA AUTO REPAIR
Certified Collision Repair
123 Main Street, Redmond, WA 98052
Phone: (425) 000-0000
Preliminary Estimate (ID: EST-1008)
| Customer Information |                     |     | Vehicle Information |                   |
| -------------------- | ------------------- | --- | ------------------- | ----------------- |
| Insured name         | Gabriel Diaz        |     | Year                | 2022              |
| Claim #              | SF-1008             |     | Make                | Jeep              |
| Policy #             | POL-2022-555        |     | Model               | Grand Cherokee    |
| Phone                | (425) 111-1111      |     | Trim                | Limited           |
| Email                | gabriel@contoso.com |     | VIN                 | 1C4RJFBG2NC123456 |
|                      |                     |     | Color               | White             |
|                      |                     |     | Odometer            | 9,800             |
| Repair Order #       | RO-20221108         |     | Estimator           | Ellis Turner      |
Estimate Totals
|                  |     | Hours | Rate | Cost  |
| ---------------- | --- | ----- | ---- | ----- |
| Parts            |     |       |      | 2,100 |
| Body Labor       |     | 2     | 150  | 300   |
| Paint Labor      |     | 1.5   | 150  | 225   |
| Mechanical Labor |     | -     | -    | -     |
Supplies
|               | Paint Supplies           |     |        | 60     |
| ------------- | ------------------------ | --- | ------ | ------ |
|               | Body Supplies            |     |        | 30     |
| Other Charges |                          |     |        | 15     |
| Subtotal      |                          |     |        | 2,730  |
| Sales Tax     |                          |     | 10.20% | 278.46 |
| GRAND TOTAL   |                          |     |        | 5,738  |
| Note          | Minor rear bumper repair |     |        |        |
This is a preliminary estimate for the visible damage of the vehicle. Additional damage / repairs / parts may be found
after the vehicle has been disassembled and damaged parts have been removed. Suspension damages may be
present, but can not be determined until an alignment on the vehicle has been done. Parts Prices may vary due to
models and vehicle maker price updates. Please be advised if vehicle owner elects to have vehicle sent to service for
any mechanical concerns, ALL service departments charge a vehicle diagnostic charge. If the mechanical concern is
deemed not related to an insurance claim, vehicle owner will be reponsible for charges.

1 Introduction
Largelanguagemodels(LLMs)arebecomingacrucialbuildingblockindevelopingpowerfulagents
thatutilizeLLMsforreasoning,toolusage,andadaptingtonewobservations(Yaoetal.,2022;Xi
et al., 2023; Wang et al., 2023b) in many real-world tasks. Given the expanding tasks that could
benefitfromLLMsandthegrowingtaskcomplexity,anintuitiveapproachtoscaleupthepowerof
agents is to use multiple agents that cooperate. Prior work suggests that multiple agents can help
encouragedivergentthinking(Liangetal.,2023),improvefactualityandreasoning(Duetal.,2023),
andprovidevalidation(Wuetal.,2023). Inlightoftheintuitionandearlyevidenceofpromise,itis
intriguingtoaskthefollowingquestion:howcanwefacilitatethedevelopmentofLLMapplications
thatcouldspanabroadspectrumofdomainsandcomplexitiesbasedonthemulti-agentapproach?
Our insight is to use multi-agent conversations to achieve it. There are at least three reasons con-
firming its general feasibility and utility thanks to recent advances in LLMs: First, because chat-
optimizedLLMs(e.g.,GPT-4)showtheabilitytoincorporatefeedback,LLMagentscancooperate
throughconversationswitheachotherorhuman(s),e.g.,adialogwhereagentsprovideandseekrea-
soning, observations, critiques, andvalidation. Second, becauseasingleLLMcanexhibitabroad
range of capabilities (especially when configured with the correct prompt and inference settings),
conversationsbetweendifferentlyconfiguredagentscanhelpcombinethesebroadLLMcapabilities
inamodularandcomplementarymanner. Third,LLMshavedemonstratedabilitytosolvecomplex
tasks when the tasks are broken into simpler subtasks. Multi-agent conversations can enable this
partitioning and integration in an intuitive manner. How can we leverage the above insights and
supportdifferentapplicationswiththecommonrequirementofcoordinatingmultipleagents,poten-
tially backed by LLMs, humans, or tools exhibiting different capacities? We desire a multi-agent
conversationframeworkwithgenericabstractionandeffectiveimplementationthathastheflexibil-
itytosatisfydifferentapplicationneeds. Achievingthisrequiresaddressingtwocriticalquestions:
(1)Howcanwedesignindividualagentsthatarecapable, reusable, customizable, andeffectivein
multi-agent collaboration? (2) How can we develop a straightforward, unified interface that can
accommodate a wide range of agent conversation patterns? In practice, applications of varying
complexities may need distinct sets of agents with specific capabilities, and may require different
conversationpatterns,suchassingle-ormulti-turndialogs,differenthumaninvolvementmodes,and
staticvs. dynamicconversation. Moreover, developersmayprefertheflexibilitytoprogramagent
interactions in natural language or code. Failing to adequately address these two questions would
limittheframework’sscopeofapplicabilityandgenerality.
While there is contemporaneous exploration of multi-agent approaches,3 we present AutoGen, a
generalizedmulti-agentconversationframework(Figure1),basedonthefollowingnewconcepts.
1 Customizableandconversableagents. AutoGenusesagenericdesignofagentsthatcanlever-
age LLMs, human inputs, tools, or a combination of them. The result is that developers can
easily and quickly create agents with different roles (e.g., agents to write code, execute code,
wireinhumanfeedback,validateoutputs,etc.) byselectingandconfiguringasubsetofbuilt-in
capabilities. Theagent’sbackendcanalsobereadilyextendedtoallowmorecustombehaviors.
To make these agents suitable for multi-agent conversation, every agent is made conversable –
theycanreceive,react,andrespondtomessages. Whenconfiguredproperly,anagentcanhold
multiple turns of conversations with other agents autonomously or solicit human inputs at cer-
tainrounds,enablinghumanagencyandautomation. Theconversableagentdesignleveragesthe
strongcapabilityofthemostadvancedLLMsintakingfeedbackandmakingprogressviachat
andalsoallowscombiningcapabilitiesofLLMsinamodularfashion. (Section2.1)
2 Conversationprogramming. AfundamentalinsightofAutoGenistosimplifyandunifycom-
plexLLMapplicationworkflowsasmulti-agentconversations. SoAutoGenadoptsaprogram-
ming paradigm centered around these inter-agent conversations. We refer to this paradigm as
conversationprogramming,whichstreamlinesthedevelopmentofintricateapplicationsviatwo
primary steps: (1) defining a set of conversable agents with specific capabilities and roles (as
described above); (2) programming the interaction behavior between agents via conversation-
centric computation and control. Both steps can be achieved via a fusion of natural and pro-
gramminglanguagestobuildapplicationswithawiderangeofconversationpatternsandagent
behaviors. AutoGenprovidesready-to-useimplementationsandalsoallowseasyextensionand
experimentationforbothsteps. (Section2.2)
3WerefertoAppendixAforadetaileddiscussion.
2

ZAVA AUTO REPAIR
Certified Collision Repair
123 Main Street, Redmond, WA 98052
Phone: (425) 000-0000
Preliminary Estimate (ID: EST-1008)
| Customer Information |                     |     | Vehicle Information |                   |
| -------------------- | ------------------- | --- | ------------------- | ----------------- |
| Insured name         | Gabriel Diaz        |     | Year                | 2022              |
| Claim #              | SF-1008             |     | Make                | Jeep              |
| Policy #             | POL-2022-555        |     | Model               | Grand Cherokee    |
| Phone                | (425) 111-1111      |     | Trim                | Limited           |
| Email                | gabriel@contoso.com |     | VIN                 | 1C4RJFBG2NC123456 |
|                      |                     |     | Color               | White             |
|                      |                     |     | Odometer            | 9,800             |
| Repair Order #       | RO-20221108         |     | Estimator           | Ellis Turner      |
Estimate Totals
|                  |     | Hours | Rate | Cost  |
| ---------------- | --- | ----- | ---- | ----- |
| Parts            |     |       |      | 2,100 |
| Body Labor       |     | 2     | 150  | 300   |
| Paint Labor      |     | 1.5   | 150  | 225   |
| Mechanical Labor |     | -     | -    | -     |
Supplies
|               | Paint Supplies           |     |        | 60     |
| ------------- | ------------------------ | --- | ------ | ------ |
|               | Body Supplies            |     |        | 30     |
| Other Charges |                          |     |        | 15     |
| Subtotal      |                          |     |        | 2,730  |
| Sales Tax     |                          |     | 10.20% | 278.46 |
| GRAND TOTAL   |                          |     |        | 5,738  |
| Note          | Minor rear bumper repair |     |        |        |
This is a preliminary estimate for the visible damage of the vehicle. Additional damage / repairs / parts may be found
after the vehicle has been disassembled and damaged parts have been removed. Suspension damages may be
present, but can not be determined until an alignment on the vehicle has been done. Parts Prices may vary due to
models and vehicle maker price updates. Please be advised if vehicle owner elects to have vehicle sent to service for
any mechanical concerns, ALL service departments charge a vehicle diagnostic charge. If the mechanical concern is
deemed not related to an insurance claim, vehicle owner will be reponsible for charges.
