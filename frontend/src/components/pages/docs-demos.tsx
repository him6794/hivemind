"use client";

import { useState, type MouseEvent } from "react";
import { AlertCircle, Bell, ChevronDown, CreditCard, Info, LogOut, Settings, User } from "lucide-react";
import { toast } from "sonner";
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/shadcn/accordion";
import { Alert, AlertDescription, AlertTitle } from "@/components/shadcn/alert";
import { Avatar, AvatarFallback } from "@/components/shadcn/avatar";
import { Badge } from "@/components/shadcn/badge";
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@/components/shadcn/breadcrumb";
import { Button } from "@/components/shadcn/button";
import { Calendar } from "@/components/shadcn/calendar";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/shadcn/card";
import { Checkbox } from "@/components/shadcn/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/shadcn/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/shadcn/dropdown-menu";
import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/components/shadcn/hover-card";
import { Input } from "@/components/shadcn/input";
import { Label } from "@/components/shadcn/label";
import { Pagination, PaginationContent, PaginationEllipsis, PaginationItem, PaginationLink, PaginationNext, PaginationPrevious } from "@/components/shadcn/pagination";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/shadcn/popover";
import { Progress } from "@/components/shadcn/progress";
import { RadioGroup, RadioGroupItem } from "@/components/shadcn/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/shadcn/select";
import { Separator } from "@/components/shadcn/separator";
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle, SheetTrigger } from "@/components/shadcn/sheet";
import { Skeleton } from "@/components/shadcn/skeleton";
import { Switch } from "@/components/shadcn/switch";
import { Table, TableBody, TableCaption, TableCell, TableHead, TableHeader, TableRow } from "@/components/shadcn/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/shadcn/tabs";
import { Textarea } from "@/components/shadcn/textarea";
import { Toaster } from "@/components/shadcn/sonner";
import { CHART_DEMOS } from "./docs-demos-charts";
import { MOTION_DEMOS } from "./docs-demos-motion";
import type { DemoEntry } from "./docs-demo-types";

// Demo links would change the hash, and the hash is the route. They stay put.
function stay(event: MouseEvent) {
  event.preventDefault();
}

function ButtonVariantsDemo() {
  return (
    <div className="flex flex-wrap items-center justify-center gap-2">
      <Button>Default</Button>
      <Button variant="secondary">Secondary</Button>
      <Button variant="outline">Outline</Button>
      <Button variant="ghost">Ghost</Button>
      <Button variant="destructive">Destructive</Button>
      <Button variant="link">Link</Button>
    </div>
  );
}

function InputDemo() {
  return (
    <div className="grid w-full max-w-sm gap-2">
      <Label htmlFor="docs-email">Email</Label>
      <Input id="docs-email" type="email" placeholder="name@example.com" />
    </div>
  );
}

function TextareaDemo() {
  return (
    <div className="grid w-full max-w-sm gap-2">
      <Label htmlFor="docs-message">Message</Label>
      <Textarea id="docs-message" placeholder="Type your message here." />
    </div>
  );
}

function CheckboxDemo() {
  const [checked, setChecked] = useState(true);
  return (
    <div className="flex items-center gap-3">
      <Checkbox id="docs-terms" checked={checked} onCheckedChange={(value) => setChecked(value === true)} />
      <Label htmlFor="docs-terms">Accept the usage rules ({checked ? "accepted" : "not accepted"})</Label>
    </div>
  );
}

const plans = [
  { value: "starter", label: "Starter" },
  { value: "pro", label: "Pro" },
  { value: "team", label: "Team" },
];

function RadioDemo() {
  return (
    <RadioGroup defaultValue="pro" className="grid gap-3">
      {plans.map((plan) => (
        <div key={plan.value} className="flex items-center gap-3">
          <RadioGroupItem value={plan.value} id={`docs-plan-${plan.value}`} />
          <Label htmlFor={`docs-plan-${plan.value}`}>{plan.label}</Label>
        </div>
      ))}
    </RadioGroup>
  );
}

function SwitchDemo() {
  return (
    <div className="flex items-center gap-3">
      <Switch id="docs-share" defaultChecked />
      <Label htmlFor="docs-share">Share this computer when idle</Label>
    </div>
  );
}

function SelectDemo() {
  return (
    <Select defaultValue="asia">
      <SelectTrigger className="w-[200px]">
        <SelectValue placeholder="Region" />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="asia">Asia</SelectItem>
        <SelectItem value="europe">Europe</SelectItem>
        <SelectItem value="americas">Americas</SelectItem>
      </SelectContent>
    </Select>
  );
}

function AlertDemo() {
  return (
    <div className="grid w-full max-w-md gap-3">
      <Alert>
        <Info />
        <AlertTitle>Heads up</AlertTitle>
        <AlertDescription>A task is charged only for the valid usage that the network confirms.</AlertDescription>
      </Alert>
      <Alert variant="destructive">
        <AlertCircle />
        <AlertTitle>Budget exhausted</AlertTitle>
        <AlertDescription>The replica stopped with budget_exhausted. Raise max_cpt or reduce the work.</AlertDescription>
      </Alert>
    </div>
  );
}

function BadgeDemo() {
  return (
    <div className="flex flex-wrap gap-2">
      <Badge>Confirmed</Badge>
      <Badge variant="secondary">Running</Badge>
      <Badge variant="outline">Queued</Badge>
      <Badge variant="destructive">Failed</Badge>
    </div>
  );
}

function ProgressDemo() {
  return (
    <div className="w-full max-w-md space-y-2">
      <div className="flex justify-between text-sm text-muted-foreground">
        <span>Upload</span>
        <span className="tabular-nums">66%</span>
      </div>
      <Progress value={66} />
    </div>
  );
}

function SkeletonDemo() {
  return (
    <div className="flex w-full max-w-sm items-center gap-4">
      <Skeleton className="size-12 rounded-full" />
      <div className="grid flex-1 gap-2">
        <Skeleton className="h-4 w-3/4" />
        <Skeleton className="h-4 w-1/2" />
      </div>
    </div>
  );
}

function ToastDemo() {
  return (
    <div className="flex flex-col items-center gap-3">
      <Button
        variant="outline"
        onClick={() => toast("Task submitted", { description: "Three replicas are being dispatched to Workers." })}
      >
        <Bell />
        Show toast
      </Button>
      <Toaster />
    </div>
  );
}

function DialogDemo() {
  return (
    <Dialog>
      <DialogTrigger asChild>
        <Button variant="outline">Edit profile</Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Edit profile</DialogTitle>
          <DialogDescription>Make changes to your profile here. Save when you are done.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-2 py-2">
          <Label htmlFor="docs-name">Name</Label>
          <Input id="docs-name" defaultValue="Ada Lovelace" />
        </div>
        <DialogFooter>
          <Button>Save changes</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function SheetDemo() {
  return (
    <Sheet>
      <SheetTrigger asChild>
        <Button variant="outline">Open sheet</Button>
      </SheetTrigger>
      <SheetContent>
        <SheetHeader>
          <SheetTitle>Worker settings</SheetTitle>
          <SheetDescription>Changes apply the next time Worker starts.</SheetDescription>
        </SheetHeader>
        <div className="flex items-center justify-between gap-4 px-4">
          <Label htmlFor="docs-sheet-share">Share when idle</Label>
          <Switch id="docs-sheet-share" defaultChecked />
        </div>
        <SheetFooter>
          <Button>Save</Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

function PopoverDemo() {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="outline">Usage estimate</Button>
      </PopoverTrigger>
      <PopoverContent className="w-72">
        <div className="space-y-1">
          <h4 className="font-medium leading-none">Three values</h4>
          <p className="text-sm text-muted-foreground">The sum over three values records 25 usage units per replica.</p>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function DropdownDemo() {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline">
          Account
          <ChevronDown />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-56" align="start">
        <DropdownMenuLabel>My account</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem>
          <User />
          Profile
        </DropdownMenuItem>
        <DropdownMenuItem>
          <CreditCard />
          Balance
        </DropdownMenuItem>
        <DropdownMenuItem>
          <Settings />
          Settings
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem className="text-destructive">
          <LogOut />
          Sign out
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function HoverCardDemo() {
  return (
    <HoverCard>
      <HoverCardTrigger asChild>
        <Button variant="link">@hivemind</Button>
      </HoverCardTrigger>
      <HoverCardContent className="w-72">
        <div className="flex gap-3">
          <Avatar>
            <AvatarFallback>HM</AvatarFallback>
          </Avatar>
          <div className="space-y-1">
            <p className="text-sm font-medium">Hivemind</p>
            <p className="text-sm text-muted-foreground">Compute tasks on spare computers, billed by confirmed work.</p>
          </div>
        </div>
      </HoverCardContent>
    </HoverCard>
  );
}

function AccordionDemo() {
  return (
    <Accordion type="single" collapsible defaultValue="charge" className="w-full max-w-md">
      <AccordionItem value="charge">
        <AccordionTrigger>Is the charge fixed?</AccordionTrigger>
        <AccordionContent>
          No. You pay for the valid usage the network confirms, and never more than the charge cap.
        </AccordionContent>
      </AccordionItem>
      <AccordionItem value="replica">
        <AccordionTrigger>What is a replica?</AccordionTrigger>
        <AccordionContent>One Worker&apos;s run of the same task. Three replicas run each task by default.</AccordionContent>
      </AccordionItem>
    </Accordion>
  );
}

function TabsDemo() {
  return (
    <Tabs defaultValue="account" className="w-full max-w-md">
      <TabsList>
        <TabsTrigger value="account">Account</TabsTrigger>
        <TabsTrigger value="password">Password</TabsTrigger>
      </TabsList>
      <TabsContent value="account">
        <Card>
          <CardContent className="pt-6 text-sm text-muted-foreground">Change your display name and email.</CardContent>
        </Card>
      </TabsContent>
      <TabsContent value="password">
        <Card>
          <CardContent className="pt-6 text-sm text-muted-foreground">Use at least 8 characters for a new password.</CardContent>
        </Card>
      </TabsContent>
    </Tabs>
  );
}

function BreadcrumbDemo() {
  return (
    <Breadcrumb>
      <BreadcrumbList>
        <BreadcrumbItem>
          <BreadcrumbLink href="#" onClick={stay}>
            Documentation
          </BreadcrumbLink>
        </BreadcrumbItem>
        <BreadcrumbSeparator />
        <BreadcrumbItem>
          <BreadcrumbLink href="#" onClick={stay}>
            Components
          </BreadcrumbLink>
        </BreadcrumbItem>
        <BreadcrumbSeparator />
        <BreadcrumbItem>
          <BreadcrumbPage>Breadcrumb</BreadcrumbPage>
        </BreadcrumbItem>
      </BreadcrumbList>
    </Breadcrumb>
  );
}

function PaginationDemo() {
  return (
    <Pagination>
      <PaginationContent>
        <PaginationItem>
          <PaginationPrevious href="#" onClick={stay} />
        </PaginationItem>
        <PaginationItem>
          <PaginationLink href="#" onClick={stay}>
            1
          </PaginationLink>
        </PaginationItem>
        <PaginationItem>
          <PaginationLink href="#" onClick={stay} isActive>
            2
          </PaginationLink>
        </PaginationItem>
        <PaginationItem>
          <PaginationLink href="#" onClick={stay}>
            3
          </PaginationLink>
        </PaginationItem>
        <PaginationItem>
          <PaginationEllipsis />
        </PaginationItem>
        <PaginationItem>
          <PaginationNext href="#" onClick={stay} />
        </PaginationItem>
      </PaginationContent>
    </Pagination>
  );
}

function CardDemo() {
  return (
    <Card className="w-full max-w-sm">
      <CardHeader>
        <CardTitle>Sign in</CardTitle>
        <CardDescription>Enter your username and password to continue.</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <div className="grid gap-2">
          <Label htmlFor="docs-card-user">Username</Label>
          <Input id="docs-card-user" defaultValue="ada" />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="docs-card-pass">Password</Label>
          <Input id="docs-card-pass" type="password" />
        </div>
      </CardContent>
      <CardFooter>
        <Button className="w-full">Sign in</Button>
      </CardFooter>
    </Card>
  );
}

const recentTasks = [
  { id: "T-1041", status: "Confirmed", units: 25, charged: "82 CPT" },
  { id: "T-1042", status: "Running", units: 12, charged: "Held" },
  { id: "T-1043", status: "No quorum", units: 9, charged: "0 CPT" },
];

function TableDemo() {
  return (
    <div className="w-full max-w-lg overflow-x-auto">
      <Table>
        <TableCaption>Recent tasks (sample data)</TableCaption>
        <TableHeader>
          <TableRow>
            <TableHead>Task</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="text-right">Usage</TableHead>
            <TableHead className="text-right">Charged</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {recentTasks.map((task) => (
            <TableRow key={task.id}>
              <TableCell className="font-medium">{task.id}</TableCell>
              <TableCell>{task.status}</TableCell>
              <TableCell className="text-right tabular-nums">{task.units}</TableCell>
              <TableCell className="text-right tabular-nums">{task.charged}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

function AvatarDemo() {
  return (
    <div className="flex items-center gap-4">
      <Avatar>
        <AvatarFallback>AL</AvatarFallback>
      </Avatar>
      <Avatar>
        <AvatarFallback className="bg-secondary">HM</AvatarFallback>
      </Avatar>
      <Avatar>
        <AvatarFallback className="bg-muted">W1</AvatarFallback>
      </Avatar>
    </div>
  );
}

function SeparatorDemo() {
  return (
    <div className="w-full max-w-sm text-sm">
      <div className="space-y-1">
        <h4 className="font-medium leading-none">Workers</h4>
        <p className="text-muted-foreground">Computers that run replicas.</p>
      </div>
      <Separator className="my-4" />
      <div className="flex h-5 items-center gap-4">
        <span>Queue</span>
        <Separator orientation="vertical" />
        <span>Settle</span>
        <Separator orientation="vertical" />
        <span>Log</span>
      </div>
    </div>
  );
}

function CalendarDemo() {
  const [date, setDate] = useState<Date | undefined>(new Date(2026, 0, 12));
  return (
    <Calendar
      mode="single"
      selected={date}
      onSelect={setDate}
      defaultMonth={new Date(2026, 0, 1)}
      className="rounded-xl border"
    />
  );
}

const UI_DEMOS: Record<string, DemoEntry> = {
  "button-variants": {
    title: { en: "Variants", zh: "樣式" },
    code: `import { Button } from "@/components/shadcn/button"

<div className="flex flex-wrap gap-2">
  <Button>Default</Button>
  <Button variant="secondary">Secondary</Button>
  <Button variant="outline">Outline</Button>
  <Button variant="ghost">Ghost</Button>
  <Button variant="destructive">Destructive</Button>
  <Button variant="link">Link</Button>
</div>`,
    Preview: ButtonVariantsDemo,
  },
  "input-basic": {
    title: { en: "Input with label", zh: "附標籤的輸入框" },
    code: `import { Input } from "@/components/shadcn/input"
import { Label } from "@/components/shadcn/label"

<div className="grid w-full max-w-sm gap-2">
  <Label htmlFor="email">Email</Label>
  <Input id="email" type="email" placeholder="name@example.com" />
</div>`,
    Preview: InputDemo,
  },
  "textarea-basic": {
    title: { en: "Textarea", zh: "多行文字" },
    code: `import { Textarea } from "@/components/shadcn/textarea"

<Textarea placeholder="Type your message here." />`,
    Preview: TextareaDemo,
  },
  "checkbox-basic": {
    title: { en: "Checkbox", zh: "核取方塊" },
    code: `import { Checkbox } from "@/components/shadcn/checkbox"

<div className="flex items-center gap-3">
  <Checkbox id="terms" checked={checked} onCheckedChange={setChecked} />
  <Label htmlFor="terms">Accept the usage rules</Label>
</div>`,
    Preview: CheckboxDemo,
  },
  "radio-group-basic": {
    title: { en: "Radio group", zh: "單選群組" },
    code: `import { RadioGroup, RadioGroupItem } from "@/components/shadcn/radio-group"

<RadioGroup defaultValue="pro" className="grid gap-3">
  <div className="flex items-center gap-3">
    <RadioGroupItem value="starter" id="starter" />
    <Label htmlFor="starter">Starter</Label>
  </div>
  <div className="flex items-center gap-3">
    <RadioGroupItem value="pro" id="pro" />
    <Label htmlFor="pro">Pro</Label>
  </div>
</RadioGroup>`,
    Preview: RadioDemo,
  },
  "switch-basic": {
    title: { en: "Switch", zh: "開關" },
    code: `import { Switch } from "@/components/shadcn/switch"

<div className="flex items-center gap-3">
  <Switch id="share" defaultChecked />
  <Label htmlFor="share">Share this computer when idle</Label>
</div>`,
    Preview: SwitchDemo,
  },
  "select-basic": {
    title: { en: "Select", zh: "選單" },
    code: `import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/shadcn/select"

<Select defaultValue="asia">
  <SelectTrigger className="w-[200px]">
    <SelectValue placeholder="Region" />
  </SelectTrigger>
  <SelectContent>
    <SelectItem value="asia">Asia</SelectItem>
    <SelectItem value="europe">Europe</SelectItem>
  </SelectContent>
</Select>`,
    Preview: SelectDemo,
  },
  "alert-basic": {
    title: { en: "Alert", zh: "提示框" },
    code: `import { Alert, AlertDescription, AlertTitle } from "@/components/shadcn/alert"

<Alert>
  <Info />
  <AlertTitle>Heads up</AlertTitle>
  <AlertDescription>A task is charged only for the valid usage that the network confirms.</AlertDescription>
</Alert>
<Alert variant="destructive">
  <AlertCircle />
  <AlertTitle>Budget exhausted</AlertTitle>
  <AlertDescription>Raise max_cpt or reduce the work.</AlertDescription>
</Alert>`,
    Preview: AlertDemo,
  },
  "badge-variants": {
    title: { en: "Badges", zh: "標籤" },
    code: `import { Badge } from "@/components/shadcn/badge"

<div className="flex flex-wrap gap-2">
  <Badge>Confirmed</Badge>
  <Badge variant="secondary">Running</Badge>
  <Badge variant="outline">Queued</Badge>
  <Badge variant="destructive">Failed</Badge>
</div>`,
    Preview: BadgeDemo,
  },
  "progress-basic": {
    title: { en: "Progress", zh: "進度" },
    code: `import { Progress } from "@/components/shadcn/progress"

<Progress value={66} />`,
    Preview: ProgressDemo,
  },
  "skeleton-basic": {
    title: { en: "Skeleton", zh: "載入占位" },
    code: `import { Skeleton } from "@/components/shadcn/skeleton"

<div className="flex items-center gap-4">
  <Skeleton className="size-12 rounded-full" />
  <div className="grid flex-1 gap-2">
    <Skeleton className="h-4 w-3/4" />
    <Skeleton className="h-4 w-1/2" />
  </div>
</div>`,
    Preview: SkeletonDemo,
  },
  "sonner-toast": {
    title: { en: "Toast", zh: "通知" },
    code: `import { toast } from "sonner"
import { Toaster } from "@/components/shadcn/sonner"

toast("Task submitted", { description: "Three replicas are being dispatched to Workers." })

// Mount once in the layout:
<Toaster />`,
    Preview: ToastDemo,
  },
  "dialog-basic": {
    title: { en: "Dialog", zh: "對話框" },
    code: `import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/shadcn/dialog"

<Dialog>
  <DialogTrigger asChild>
    <Button variant="outline">Edit profile</Button>
  </DialogTrigger>
  <DialogContent>
    <DialogHeader>
      <DialogTitle>Edit profile</DialogTitle>
      <DialogDescription>Make changes to your profile here.</DialogDescription>
    </DialogHeader>
    <DialogFooter>
      <Button>Save changes</Button>
    </DialogFooter>
  </DialogContent>
</Dialog>`,
    Preview: DialogDemo,
  },
  "sheet-basic": {
    title: { en: "Sheet", zh: "側邊面板" },
    code: `import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle, SheetTrigger } from "@/components/shadcn/sheet"

<Sheet>
  <SheetTrigger asChild>
    <Button variant="outline">Open sheet</Button>
  </SheetTrigger>
  <SheetContent>
    <SheetHeader>
      <SheetTitle>Worker settings</SheetTitle>
      <SheetDescription>Changes apply the next time Worker starts.</SheetDescription>
    </SheetHeader>
    <SheetFooter>
      <Button>Save</Button>
    </SheetFooter>
  </SheetContent>
</Sheet>`,
    Preview: SheetDemo,
  },
  "popover-basic": {
    title: { en: "Popover", zh: "彈出面板" },
    code: `import { Popover, PopoverContent, PopoverTrigger } from "@/components/shadcn/popover"

<Popover>
  <PopoverTrigger asChild>
    <Button variant="outline">Usage estimate</Button>
  </PopoverTrigger>
  <PopoverContent className="w-72">
    <p className="text-sm text-muted-foreground">The sum over three values records 25 usage units per replica.</p>
  </PopoverContent>
</Popover>`,
    Preview: PopoverDemo,
  },
  "dropdown-menu-basic": {
    title: { en: "Dropdown menu", zh: "下拉選單" },
    code: `import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/shadcn/dropdown-menu"

<DropdownMenu>
  <DropdownMenuTrigger asChild>
    <Button variant="outline">Account</Button>
  </DropdownMenuTrigger>
  <DropdownMenuContent align="start">
    <DropdownMenuLabel>My account</DropdownMenuLabel>
    <DropdownMenuSeparator />
    <DropdownMenuItem>Profile</DropdownMenuItem>
    <DropdownMenuItem>Settings</DropdownMenuItem>
  </DropdownMenuContent>
</DropdownMenu>`,
    Preview: DropdownDemo,
  },
  "hover-card-basic": {
    title: { en: "Hover card", zh: "懸停卡片" },
    code: `import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/components/shadcn/hover-card"

<HoverCard>
  <HoverCardTrigger asChild>
    <Button variant="link">@hivemind</Button>
  </HoverCardTrigger>
  <HoverCardContent className="w-72">
    Compute tasks on spare computers, billed by confirmed work.
  </HoverCardContent>
</HoverCard>`,
    Preview: HoverCardDemo,
  },
  "accordion-basic": {
    title: { en: "Accordion", zh: "手風琴" },
    code: `import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/shadcn/accordion"

<Accordion type="single" collapsible defaultValue="charge">
  <AccordionItem value="charge">
    <AccordionTrigger>Is the charge fixed?</AccordionTrigger>
    <AccordionContent>No. You pay for the valid usage the network confirms.</AccordionContent>
  </AccordionItem>
</Accordion>`,
    Preview: AccordionDemo,
  },
  "tabs-basic": {
    title: { en: "Tabs", zh: "分頁" },
    code: `import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/shadcn/tabs"

<Tabs defaultValue="account">
  <TabsList>
    <TabsTrigger value="account">Account</TabsTrigger>
    <TabsTrigger value="password">Password</TabsTrigger>
  </TabsList>
  <TabsContent value="account">Change your display name and email.</TabsContent>
  <TabsContent value="password">Use at least 8 characters.</TabsContent>
</Tabs>`,
    Preview: TabsDemo,
  },
  "breadcrumb-basic": {
    title: { en: "Breadcrumb", zh: "麵包屑" },
    code: `import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@/components/shadcn/breadcrumb"

<Breadcrumb>
  <BreadcrumbList>
    <BreadcrumbItem><BreadcrumbLink href="/docs">Documentation</BreadcrumbLink></BreadcrumbItem>
    <BreadcrumbSeparator />
    <BreadcrumbItem><BreadcrumbPage>Breadcrumb</BreadcrumbPage></BreadcrumbItem>
  </BreadcrumbList>
</Breadcrumb>`,
    Preview: BreadcrumbDemo,
  },
  "pagination-basic": {
    title: { en: "Pagination", zh: "分頁導覽" },
    code: `import { Pagination, PaginationContent, PaginationEllipsis, PaginationItem, PaginationLink, PaginationNext, PaginationPrevious } from "@/components/shadcn/pagination"

<Pagination>
  <PaginationContent>
    <PaginationItem><PaginationPrevious href="#" /></PaginationItem>
    <PaginationItem><PaginationLink href="#">1</PaginationLink></PaginationItem>
    <PaginationItem><PaginationLink href="#" isActive>2</PaginationLink></PaginationItem>
    <PaginationItem><PaginationEllipsis /></PaginationItem>
    <PaginationItem><PaginationNext href="#" /></PaginationItem>
  </PaginationContent>
</Pagination>`,
    Preview: PaginationDemo,
  },
  "card-basic": {
    title: { en: "Card", zh: "卡片" },
    code: `import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/shadcn/card"

<Card className="w-full max-w-sm">
  <CardHeader>
    <CardTitle>Sign in</CardTitle>
    <CardDescription>Enter your username and password to continue.</CardDescription>
  </CardHeader>
  <CardContent>...</CardContent>
  <CardFooter>
    <Button className="w-full">Sign in</Button>
  </CardFooter>
</Card>`,
    Preview: CardDemo,
  },
  "table-basic": {
    title: { en: "Table", zh: "表格" },
    code: `import { Table, TableBody, TableCaption, TableCell, TableHead, TableHeader, TableRow } from "@/components/shadcn/table"

<Table>
  <TableCaption>Recent tasks</TableCaption>
  <TableHeader>
    <TableRow>
      <TableHead>Task</TableHead>
      <TableHead className="text-right">Charged</TableHead>
    </TableRow>
  </TableHeader>
  <TableBody>
    <TableRow>
      <TableCell className="font-medium">T-1041</TableCell>
      <TableCell className="text-right">82 CPT</TableCell>
    </TableRow>
  </TableBody>
</Table>`,
    Preview: TableDemo,
  },
  "avatar-basic": {
    title: { en: "Avatar", zh: "頭像" },
    code: `import { Avatar, AvatarFallback } from "@/components/shadcn/avatar"

<Avatar>
  <AvatarFallback>AL</AvatarFallback>
</Avatar>`,
    Preview: AvatarDemo,
  },
  "separator-basic": {
    title: { en: "Separator", zh: "分隔線" },
    code: `import { Separator } from "@/components/shadcn/separator"

<Separator className="my-4" />

<div className="flex h-5 items-center gap-4">
  <span>Queue</span>
  <Separator orientation="vertical" />
  <span>Settle</span>
</div>`,
    Preview: SeparatorDemo,
  },
  "calendar-basic": {
    title: { en: "Calendar", zh: "日曆" },
    code: `import { Calendar } from "@/components/shadcn/calendar"

const [date, setDate] = useState<Date | undefined>(new Date(2026, 0, 12))

<Calendar mode="single" selected={date} onSelect={setDate} className="rounded-xl border" />`,
    Preview: CalendarDemo,
  },
};

export const DEMOS: Record<string, DemoEntry> = {
  ...UI_DEMOS,
  ...CHART_DEMOS,
  ...MOTION_DEMOS,
};
